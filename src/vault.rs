use crate::{
    config::BackupRetentionConfig,
    search::SearchDocument,
    secure_fs,
    sync::{self, FolderBackend, SyncBackend},
};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rand::{RngCore, rngs::OsRng};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashSet},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use thiserror::Error;
use zeroize::Zeroizing;

const FILE_MAGIC: [u8; 4] = *b"BSJE";
const FILE_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;

const DEFAULT_MEMORY_KIB: u32 = 65_536;
const DEFAULT_ITERATIONS: u32 = 3;
const DEFAULT_PARALLELISM: u32 = 1;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("argon2 error: {0}")]
    Argon2(String),
    #[error("invalid vault format: {0}")]
    InvalidFormat(String),
    #[error("encryption failed")]
    EncryptionFailed,
    #[error("decryption failed")]
    DecryptionFailed,
    #[error("invalid date: {0}")]
    InvalidDate(String),
    #[error("sync error: {0}")]
    Sync(String),
}

pub type VaultResult<T> = Result<T, VaultError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: String,
    #[serde(rename = "memoryKiB")]
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    #[serde(rename = "saltHex")]
    pub salt_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultOptions {
    #[serde(rename = "epochDate")]
    pub epoch_date: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultMetadata {
    pub version: u32,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "deviceId")]
    pub device_id: String,
    pub kdf: KdfParams,
    pub options: VaultOptions,
}

impl VaultMetadata {
    pub fn epoch_date(&self) -> VaultResult<NaiveDate> {
        NaiveDate::parse_from_str(&self.options.epoch_date, "%Y-%m-%d")
            .map_err(|_| VaultError::InvalidDate(self.options.epoch_date.clone()))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub nickname: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictHead {
    pub revision_hash: String,
    pub device_id: String,
    pub seq: u64,
    pub saved_at: DateTime<Utc>,
    pub body: String,
    pub closing_thought: Option<String>,
    pub entry_metadata: EntryMetadata,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictState {
    pub date: NaiveDate,
    pub heads: Vec<ConflictHead>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMetadata {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub people: Vec<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub mood: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct LoadedDateState {
    pub revision_text: Option<String>,
    pub revision_closing_thought: Option<String>,
    pub revision_entry_metadata: EntryMetadata,
    pub recovery_draft_text: Option<String>,
    pub recovery_draft_closing_thought: Option<String>,
    pub recovery_draft_entry_metadata: Option<EntryMetadata>,
    pub conflict: Option<ConflictState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub date: NaiveDate,
    pub entry_number: String,
    pub preview: String,
    pub has_conflict: bool,
    pub metadata: EntryMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncReport {
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: Vec<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupSummary {
    pub path: PathBuf,
    pub pruned: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupEntry {
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedEntry {
    pub date: NaiveDate,
    pub entry_number: String,
    pub body: String,
    pub closing_thought: Option<String>,
    pub metadata: EntryMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityIssue {
    pub date: Option<NaiveDate>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrityReport {
    pub ok: bool,
    pub issues: Vec<IntegrityIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchCacheStatus {
    pub path: PathBuf,
    pub exists: bool,
    pub size_bytes: u64,
    pub modified_at: Option<DateTime<Utc>>,
    pub entry_count: Option<usize>,
    pub valid: bool,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSummary {
    pub total_entries: usize,
    pub streak_days: usize,
    pub entries_this_week: usize,
    pub entries_this_month: usize,
    pub on_this_day: Vec<ReviewHit>,
    pub top_tags: Vec<(String, usize)>,
    pub top_people: Vec<(String, usize)>,
    pub top_projects: Vec<(String, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewHit {
    pub date: NaiveDate,
    pub entry_number: String,
    pub preview: String,
}

#[derive(Clone, Debug)]
struct DraftInfo {
    body: String,
    closing_thought: Option<String>,
    entry_metadata: EntryMetadata,
    saved_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct CandidateRevision {
    path: PathBuf,
    device_id: String,
    seq: u64,
}

#[derive(Clone, Debug)]
struct RevisionRecord {
    path: PathBuf,
    revision_hash: String,
    device_id: String,
    seq: u64,
    saved_at: DateTime<Utc>,
    prev_hash: Option<String>,
    merged_hashes: Vec<String>,
    body: String,
    closing_thought: Option<String>,
    entry_metadata: EntryMetadata,
}

#[derive(Clone, Debug)]
struct CatalogEntry {
    date: NaiveDate,
    entry_number: String,
    preview: String,
    has_conflict: bool,
    search_text: String,
    metadata: EntryMetadata,
}

struct SaveRevisionSpec<'a> {
    body: &'a str,
    closing_thought: Option<&'a str>,
    entry_metadata: &'a EntryMetadata,
    prev_hash: Option<String>,
    merged_hashes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct RevisionFile {
    pub version: u8,
    pub nonce: [u8; NONCE_LEN],
    pub ciphertext: Vec<u8>,
}

impl RevisionFile {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + 1 + NONCE_LEN + self.ciphertext.len());
        bytes.extend_from_slice(&FILE_MAGIC);
        bytes.push(self.version);
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    pub fn parse(bytes: &[u8]) -> VaultResult<Self> {
        let minimum_len = FILE_MAGIC.len() + 1 + NONCE_LEN + 16;
        if bytes.len() < minimum_len {
            return Err(VaultError::InvalidFormat(
                "encrypted file too short".to_string(),
            ));
        }
        if bytes[0..4] != FILE_MAGIC {
            return Err(VaultError::InvalidFormat("invalid magic".to_string()));
        }
        let version = bytes[4];
        if version != FILE_VERSION {
            return Err(VaultError::InvalidFormat(format!(
                "unsupported file version {version}"
            )));
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[5..5 + NONCE_LEN]);
        Ok(Self {
            version,
            nonce,
            ciphertext: bytes[5 + NONCE_LEN..].to_vec(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct RevisionPayload {
    kind: String,
    #[serde(rename = "savedAt")]
    saved_at: String,
    date: String,
    body: String,
    seq: u64,
    #[serde(rename = "deviceId")]
    device_id: String,
    #[serde(rename = "prevHash", default)]
    prev_hash: Option<String>,
    #[serde(rename = "mergedHashes", default)]
    merged_hashes: Vec<String>,
    #[serde(rename = "closingThought", default)]
    closing_thought: Option<String>,
    #[serde(rename = "entryMetadata", default)]
    entry_metadata: EntryMetadata,
}

#[derive(Serialize, Deserialize)]
struct DraftPayload {
    kind: String,
    #[serde(rename = "savedAt")]
    saved_at: String,
    date: String,
    body: String,
    #[serde(rename = "deviceId")]
    device_id: String,
    #[serde(rename = "closingThought", default)]
    closing_thought: Option<String>,
    #[serde(rename = "entryMetadata", default)]
    entry_metadata: EntryMetadata,
}

#[derive(Serialize, Deserialize)]
struct SearchCachePayload {
    kind: String,
    #[serde(rename = "fingerprint")]
    fingerprint: Vec<CacheFingerprintEntry>,
    entries: Vec<SearchCacheEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SearchCacheEntry {
    date: String,
    entry_number: String,
    preview: String,
    has_conflict: bool,
    search_text: String,
    #[serde(default)]
    metadata: EntryMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CacheFingerprintEntry {
    path: String,
    size: u64,
    modified_unix_secs: u64,
}

pub struct UnlockedVault {
    root: PathBuf,
    metadata: VaultMetadata,
    key: Zeroizing<Vec<u8>>,
    device_id: String,
}

impl UnlockedVault {
    pub fn metadata(&self) -> &VaultMetadata {
        &self.metadata
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_revision(&self, date: NaiveDate, body: &str) -> VaultResult<()> {
        self.save_entry_revision(date, body, None, &EntryMetadata::default())
    }

    pub fn save_entry_revision(
        &self,
        date: NaiveDate,
        body: &str,
        closing_thought: Option<&str>,
        entry_metadata: &EntryMetadata,
    ) -> VaultResult<()> {
        let records = self.scan_date_revisions(date)?;
        let heads = head_records(&records);
        let prev_hash = heads.first().map(|record| record.revision_hash.clone());
        self.save_revision_internal(
            date,
            SaveRevisionSpec {
                body,
                closing_thought,
                entry_metadata,
                prev_hash,
                merged_hashes: Vec::new(),
            },
            &records,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_merge_revision(
        &self,
        date: NaiveDate,
        body: &str,
        primary_hash: &str,
        merged_hashes: &[String],
    ) -> VaultResult<()> {
        self.save_entry_merge_revision(
            date,
            body,
            None,
            &EntryMetadata::default(),
            primary_hash,
            merged_hashes,
        )
    }

    pub fn save_entry_merge_revision(
        &self,
        date: NaiveDate,
        body: &str,
        closing_thought: Option<&str>,
        entry_metadata: &EntryMetadata,
        primary_hash: &str,
        merged_hashes: &[String],
    ) -> VaultResult<()> {
        let records = self.scan_date_revisions(date)?;
        self.save_revision_internal(
            date,
            SaveRevisionSpec {
                body,
                closing_thought,
                entry_metadata,
                prev_hash: Some(primary_hash.to_string()),
                merged_hashes: merged_hashes.to_vec(),
            },
            &records,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_draft(&self, date: NaiveDate, body: &str) -> VaultResult<()> {
        self.save_entry_draft(date, body, None, &EntryMetadata::default())
    }

    pub fn save_entry_draft(
        &self,
        date: NaiveDate,
        body: &str,
        closing_thought: Option<&str>,
        entry_metadata: &EntryMetadata,
    ) -> VaultResult<()> {
        let date_directory = date_dir(&self.root, date);
        let year_directory = date_directory
            .parent()
            .ok_or_else(|| VaultError::InvalidFormat("missing year directory".to_string()))?;
        secure_fs::ensure_private_dir(year_directory)?;
        secure_fs::ensure_private_dir(&date_directory)?;
        let payload = DraftPayload {
            kind: "draft".to_string(),
            saved_at: Utc::now().to_rfc3339(),
            date: date.format("%Y-%m-%d").to_string(),
            body: body.to_string(),
            device_id: self.device_id.clone(),
            closing_thought: normalize_optional_text(closing_thought),
            entry_metadata: sanitize_entry_metadata(entry_metadata),
        };
        let plaintext = serde_json::to_vec(&payload)?;
        let aad = aad_string("draft", date, &self.device_id, 0);
        let encrypted = encrypt_payload(&self.key, &plaintext, aad.as_bytes())?;
        let path = date_directory.join(draft_file_name(&self.device_id));
        atomic_write(&path, &encrypted.to_bytes())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn export_entry_text(&self, date: NaiveDate) -> VaultResult<Option<String>> {
        Ok(self
            .export_entry(date)?
            .map(|entry| format_export_text(&entry.body, entry.closing_thought.as_deref())))
    }

    pub fn export_entry(&self, date: NaiveDate) -> VaultResult<Option<ExportedEntry>> {
        let records = self.scan_date_revisions(date)?;
        let heads = head_records(&records);
        let primary = heads.first().or_else(|| records.first());
        let Some(record) = primary else {
            return Ok(None);
        };
        let epoch = self.metadata.epoch_date()?;
        Ok(Some(ExportedEntry {
            date,
            entry_number: compute_entry_number(epoch, date),
            body: record.body.clone(),
            closing_thought: record.closing_thought.clone(),
            metadata: record.entry_metadata.clone(),
        }))
    }

    pub fn create_backup(&self, retention: &BackupRetentionConfig) -> VaultResult<BackupSummary> {
        let created_at = Utc::now();
        let archive_bytes = build_backup_archive(&self.root)?;
        let compressed = zstd::stream::encode_all(Cursor::new(archive_bytes), 3)?;
        let encrypted = encrypt_payload(
            &self.key,
            &compressed,
            backup_aad_string(created_at).as_bytes(),
        )?;

        let backup_dir = self.root.join("backups");
        secure_fs::ensure_private_dir(&backup_dir)?;
        let path = backup_dir.join(backup_file_name(created_at));
        atomic_write(&path, &encrypted.to_bytes())?;
        let pruned = prune_backups(&backup_dir, retention)?;
        Ok(BackupSummary { path, pruned })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn restore_backup_into(&self, backup_path: &Path, target_root: &Path) -> VaultResult<()> {
        let timestamp = parse_backup_timestamp(backup_path)?;
        let encrypted = RevisionFile::parse(&fs::read(backup_path)?)?;
        let compressed = decrypt_payload(
            &self.key,
            &encrypted,
            backup_aad_string(timestamp).as_bytes(),
        )?;
        let archive_bytes = zstd::stream::decode_all(Cursor::new(compressed))?;
        secure_fs::ensure_private_dir(target_root)?;
        unpack_backup_archive_safely(&archive_bytes, target_root)?;
        Ok(())
    }

    pub fn load_date_state(&self, date: NaiveDate) -> VaultResult<LoadedDateState> {
        let records = self.scan_date_revisions(date)?;
        let heads = head_records(&records);
        let primary = heads.first().or_else(|| records.first());
        let revision_text = primary.map(|record| record.body.clone());
        let revision_closing_thought = primary.and_then(|record| record.closing_thought.clone());
        let revision_entry_metadata = primary
            .map(|record| record.entry_metadata.clone())
            .unwrap_or_default();

        let conflict = if heads.len() > 1 {
            Some(ConflictState {
                date,
                heads: heads
                    .iter()
                    .map(|record| ConflictHead {
                        revision_hash: record.revision_hash.clone(),
                        device_id: record.device_id.clone(),
                        seq: record.seq,
                        saved_at: record.saved_at,
                        body: record.body.clone(),
                        closing_thought: record.closing_thought.clone(),
                        entry_metadata: record.entry_metadata.clone(),
                        preview: entry_preview(&record.body, record.closing_thought.as_deref(), 54),
                    })
                    .collect(),
            })
        } else {
            None
        };

        let draft = self.read_draft(date)?;
        let (recovery_draft_text, recovery_draft_closing_thought, recovery_draft_entry_metadata) =
            match (draft, primary) {
                (Some(draft), Some(revision)) if draft.saved_at > revision.saved_at => (
                    Some(draft.body),
                    draft.closing_thought,
                    Some(draft.entry_metadata),
                ),
                (Some(draft), None) => (
                    Some(draft.body),
                    draft.closing_thought,
                    Some(draft.entry_metadata),
                ),
                _ => (None, None, None),
            };

        Ok(LoadedDateState {
            revision_text,
            revision_closing_thought,
            revision_entry_metadata,
            recovery_draft_text,
            recovery_draft_closing_thought,
            recovery_draft_entry_metadata,
            conflict,
        })
    }

    pub fn list_entry_dates(&self) -> VaultResult<Vec<NaiveDate>> {
        let entries_root = self.root.join("entries");
        if !entries_root.exists() {
            return Ok(Vec::new());
        }

        let mut dates = BTreeSet::new();
        for year_entry in fs::read_dir(entries_root)? {
            let year_entry = year_entry?;
            if !year_entry.file_type()?.is_dir() {
                continue;
            }
            for date_entry in fs::read_dir(year_entry.path())? {
                let date_entry = date_entry?;
                if !date_entry.file_type()?.is_dir() {
                    continue;
                }
                let file_name = date_entry.file_name().to_string_lossy().to_string();
                let Ok(date) = NaiveDate::parse_from_str(&file_name, "%Y-%m-%d") else {
                    continue;
                };
                if !list_revision_candidates(&date_entry.path())?.is_empty() {
                    dates.insert(date);
                }
            }
        }

        Ok(dates.into_iter().collect())
    }

    pub fn list_conflicted_dates(&self) -> VaultResult<Vec<NaiveDate>> {
        let mut conflicts = Vec::new();
        for date in self.list_entry_dates()? {
            let records = self.scan_date_revisions(date)?;
            if head_records(&records).len() > 1 {
                conflicts.push(date);
            }
        }
        Ok(conflicts)
    }

    pub fn list_index_entries(&self, preview_chars: usize) -> VaultResult<Vec<IndexEntry>> {
        let entries = self.load_catalog_entries()?;
        Ok(entries
            .into_iter()
            .map(|entry| IndexEntry {
                date: entry.date,
                entry_number: entry.entry_number,
                preview: truncate_chars(&entry.preview, preview_chars),
                has_conflict: entry.has_conflict,
                metadata: entry.metadata,
            })
            .collect())
    }

    pub fn search_cache_status(&self) -> SearchCacheStatus {
        let path = search_cache_path(&self.root);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                return SearchCacheStatus {
                    path,
                    exists: false,
                    size_bytes: 0,
                    modified_at: None,
                    entry_count: None,
                    valid: false,
                    issue: None,
                };
            }
        };

        let size_bytes = metadata.len();
        let modified_at = metadata.modified().ok().map(DateTime::<Utc>::from);

        let (entry_count, valid, issue) = match fs::read(&path)
            .map_err(VaultError::from)
            .and_then(|bytes| RevisionFile::parse(&bytes))
            .and_then(|encrypted| {
                decrypt_payload(&self.key, &encrypted, cache_aad_string().as_bytes())
            })
            .and_then(|plaintext| {
                serde_json::from_slice::<SearchCachePayload>(&plaintext).map_err(VaultError::from)
            }) {
            Ok(payload) if payload.kind == "search-cache" => {
                (Some(payload.entries.len()), true, None)
            }
            Ok(payload) => (
                Some(payload.entries.len()),
                false,
                Some(format!("unexpected cache kind '{}'", payload.kind)),
            ),
            Err(error) => (None, false, Some(error.to_string())),
        };

        SearchCacheStatus {
            path,
            exists: true,
            size_bytes,
            modified_at,
            entry_count,
            valid,
            issue,
        }
    }

    pub fn load_search_documents(&self) -> VaultResult<Vec<SearchDocument>> {
        Ok(self
            .load_catalog_entries()?
            .into_iter()
            .map(|entry| SearchDocument {
                date: entry.date,
                entry_number: entry.entry_number,
                body: entry.search_text,
            })
            .collect())
    }

    fn load_catalog_entries(&self) -> VaultResult<Vec<CatalogEntry>> {
        let fingerprint = build_catalog_fingerprint(&self.root)?;
        if let Some(entries) = self.read_catalog_cache(&fingerprint)? {
            return Ok(entries);
        }

        let epoch = self.metadata.epoch_date()?;
        let mut dates = self.list_entry_dates()?;
        dates.sort_unstable_by(|left, right| right.cmp(left));

        let mut entries = Vec::with_capacity(dates.len());
        for date in dates {
            let records = self.scan_date_revisions(date)?;
            let heads = head_records(&records);
            let primary = heads.first().or_else(|| records.first());
            if let Some(record) = primary {
                let metadata = sanitize_entry_metadata(&record.entry_metadata);
                entries.push(CatalogEntry {
                    date,
                    entry_number: compute_entry_number(epoch, date),
                    preview: entry_preview(
                        record.body.as_str(),
                        record.closing_thought.as_deref(),
                        120,
                    ),
                    has_conflict: heads.len() > 1,
                    search_text: metadata_search_text(
                        &record.body,
                        record.closing_thought.as_deref(),
                        &metadata,
                    ),
                    metadata,
                });
            }
        }

        self.write_catalog_cache(&fingerprint, &entries)?;
        Ok(entries)
    }

    fn read_catalog_cache(
        &self,
        fingerprint: &[CacheFingerprintEntry],
    ) -> VaultResult<Option<Vec<CatalogEntry>>> {
        let path = search_cache_path(&self.root);
        if !path.exists() {
            return Ok(None);
        }
        let encrypted = RevisionFile::parse(&fs::read(&path)?)?;
        let plaintext = decrypt_payload(&self.key, &encrypted, cache_aad_string().as_bytes())?;
        let payload: SearchCachePayload = serde_json::from_slice(&plaintext)?;
        if payload.kind != "search-cache" || payload.fingerprint != fingerprint {
            return Ok(None);
        }

        let mut entries = Vec::with_capacity(payload.entries.len());
        for entry in payload.entries {
            let date = NaiveDate::parse_from_str(&entry.date, "%Y-%m-%d")
                .map_err(|_| VaultError::InvalidDate(entry.date.clone()))?;
            entries.push(CatalogEntry {
                date,
                entry_number: entry.entry_number,
                preview: entry.preview,
                has_conflict: entry.has_conflict,
                search_text: entry.search_text,
                metadata: sanitize_entry_metadata(&entry.metadata),
            });
        }
        Ok(Some(entries))
    }

    fn write_catalog_cache(
        &self,
        fingerprint: &[CacheFingerprintEntry],
        entries: &[CatalogEntry],
    ) -> VaultResult<()> {
        let cache_path = search_cache_path(&self.root);
        let cache_dir = cache_path
            .parent()
            .ok_or_else(|| VaultError::InvalidFormat("missing cache directory".to_string()))?;
        secure_fs::ensure_private_dir(cache_dir)?;
        let payload = SearchCachePayload {
            kind: "search-cache".to_string(),
            fingerprint: fingerprint.to_vec(),
            entries: entries
                .iter()
                .map(|entry| SearchCacheEntry {
                    date: entry.date.format("%Y-%m-%d").to_string(),
                    entry_number: entry.entry_number.clone(),
                    preview: entry.preview.clone(),
                    has_conflict: entry.has_conflict,
                    search_text: entry.search_text.clone(),
                    metadata: entry.metadata.clone(),
                })
                .collect(),
        };
        let plaintext = serde_json::to_vec(&payload)?;
        let encrypted = encrypt_payload(&self.key, &plaintext, cache_aad_string().as_bytes())?;
        atomic_write(&cache_path, &encrypted.to_bytes())
    }

    pub fn review_summary(&self, today: NaiveDate) -> VaultResult<ReviewSummary> {
        let entries = self.load_catalog_entries()?;
        let total_entries = entries.len();
        let entries_this_week = entries
            .iter()
            .filter(|entry| (today - entry.date).num_days().abs() < 7 && entry.date <= today)
            .count();
        let entries_this_month = entries
            .iter()
            .filter(|entry| {
                entry.date.year() == today.year() && entry.date.month() == today.month()
            })
            .count();

        let mut streak_days = 0usize;
        let mut cursor = today;
        let entry_dates = entries
            .iter()
            .map(|entry| entry.date)
            .collect::<HashSet<_>>();
        while entry_dates.contains(&cursor) {
            streak_days += 1;
            cursor -= chrono::Duration::days(1);
        }

        let mut on_this_day = entries
            .iter()
            .filter(|entry| {
                entry.date.month() == today.month()
                    && entry.date.day() == today.day()
                    && entry.date.year() != today.year()
            })
            .map(|entry| ReviewHit {
                date: entry.date,
                entry_number: entry.entry_number.clone(),
                preview: entry.preview.clone(),
            })
            .collect::<Vec<_>>();
        on_this_day.sort_by(|left, right| right.date.cmp(&left.date));

        Ok(ReviewSummary {
            total_entries,
            streak_days,
            entries_this_week,
            entries_this_month,
            on_this_day,
            top_tags: top_counts(entries.iter().flat_map(|entry| entry.metadata.tags.iter())),
            top_people: top_counts(
                entries
                    .iter()
                    .flat_map(|entry| entry.metadata.people.iter()),
            ),
            top_projects: top_counts(
                entries
                    .iter()
                    .filter_map(|entry| entry.metadata.project.as_ref()),
            ),
        })
    }

    pub fn verify_integrity(&self) -> VaultResult<IntegrityReport> {
        let mut issues = Vec::new();
        for date in self.list_entry_dates()? {
            match self.scan_date_revisions(date) {
                Ok(records) => issues.extend(verify_records_for_date(date, &records)),
                Err(error) => issues.push(IntegrityIssue {
                    date: Some(date),
                    message: error.to_string(),
                }),
            }
        }
        Ok(IntegrityReport {
            ok: issues.is_empty(),
            issues,
        })
    }

    pub fn sync_with_backend<B: SyncBackend>(&self, backend: &mut B) -> VaultResult<SyncReport> {
        let report = sync::sync_root(&self.root, backend)?;
        let conflicts = self.list_conflicted_dates()?;
        Ok(SyncReport {
            pulled: report.pulled,
            pushed: report.pushed,
            conflicts,
        })
    }

    pub fn sync_folder(&self, remote_root: &Path) -> VaultResult<SyncReport> {
        let mut backend = FolderBackend::new(remote_root.to_path_buf());
        self.sync_with_backend(&mut backend)
    }

    pub fn list_backups(&self) -> VaultResult<Vec<BackupEntry>> {
        let backup_dir = self.root.join("backups");
        list_backup_artifacts(&backup_dir)?
            .into_iter()
            .map(|artifact| backup_artifact_to_entry(&artifact))
            .collect()
    }

    pub fn preview_backup_prune(
        &self,
        retention: &BackupRetentionConfig,
    ) -> VaultResult<Vec<BackupEntry>> {
        let backup_dir = self.root.join("backups");
        prune_candidates(&backup_dir, retention)?
            .into_iter()
            .map(|artifact| backup_artifact_to_entry(&artifact))
            .collect()
    }

    pub fn prune_backups_now(
        &self,
        retention: &BackupRetentionConfig,
    ) -> VaultResult<Vec<BackupEntry>> {
        let backup_dir = self.root.join("backups");
        let candidates = prune_candidates(&backup_dir, retention)?;
        let mut pruned = Vec::with_capacity(candidates.len());
        for artifact in candidates {
            let entry = backup_artifact_to_entry(&artifact)?;
            fs::remove_file(&artifact.path)?;
            pruned.push(entry);
        }
        Ok(pruned)
    }

    fn save_revision_internal(
        &self,
        date: NaiveDate,
        spec: SaveRevisionSpec<'_>,
        existing_records: &[RevisionRecord],
    ) -> VaultResult<()> {
        let SaveRevisionSpec {
            body,
            closing_thought,
            entry_metadata,
            prev_hash,
            mut merged_hashes,
        } = spec;
        let date_directory = date_dir(&self.root, date);
        let year_directory = date_directory
            .parent()
            .ok_or_else(|| VaultError::InvalidFormat("missing year directory".to_string()))?;
        secure_fs::ensure_private_dir(year_directory)?;
        secure_fs::ensure_private_dir(&date_directory)?;

        if let Some(prev_hash) = &prev_hash
            && !existing_records
                .iter()
                .any(|record| record.revision_hash == *prev_hash)
        {
            return Err(VaultError::InvalidFormat(
                "merge/save previous hash not found".to_string(),
            ));
        }

        merged_hashes.sort();
        merged_hashes.dedup();
        if let Some(prev_hash) = &prev_hash {
            merged_hashes.retain(|hash| hash != prev_hash);
        }
        for merged_hash in &merged_hashes {
            if !existing_records
                .iter()
                .any(|record| record.revision_hash == *merged_hash)
            {
                return Err(VaultError::InvalidFormat(
                    "merge hash not found".to_string(),
                ));
            }
        }

        let seq = next_revision_sequence_for_device(existing_records, &self.device_id);
        let payload = RevisionPayload {
            kind: "revision".to_string(),
            saved_at: Utc::now().to_rfc3339(),
            date: date.format("%Y-%m-%d").to_string(),
            body: body.to_string(),
            seq,
            device_id: self.device_id.clone(),
            prev_hash,
            merged_hashes,
            closing_thought: normalize_optional_text(closing_thought),
            entry_metadata: sanitize_entry_metadata(entry_metadata),
        };
        let plaintext = serde_json::to_vec(&payload)?;
        let aad = aad_string("revision", date, &self.device_id, seq);
        let encrypted = encrypt_payload(&self.key, &plaintext, aad.as_bytes())?;
        let path = date_directory.join(revision_file_name(&self.device_id, seq));
        atomic_write(&path, &encrypted.to_bytes())
    }

    fn scan_date_revisions(&self, date: NaiveDate) -> VaultResult<Vec<RevisionRecord>> {
        scan_date_revisions(&self.root, &self.key, date)
    }

    fn read_draft(&self, date: NaiveDate) -> VaultResult<Option<DraftInfo>> {
        let path = date_dir(&self.root, date).join(draft_file_name(&self.device_id));
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        let encrypted = RevisionFile::parse(&bytes)?;
        let aad = aad_string("draft", date, &self.device_id, 0);
        let plaintext = decrypt_payload(&self.key, &encrypted, aad.as_bytes())?;
        let payload: DraftPayload = serde_json::from_slice(&plaintext)?;
        Ok(Some(DraftInfo {
            body: payload.body,
            closing_thought: payload.closing_thought,
            entry_metadata: payload.entry_metadata,
            saved_at: parse_saved_at(&payload.saved_at)?,
        }))
    }
}

pub fn vault_exists(path: &Path) -> bool {
    path.join("vault.json").is_file()
}

pub fn load_vault_metadata(path: &Path) -> VaultResult<VaultMetadata> {
    let metadata: VaultMetadata = serde_json::from_slice(&fs::read(path.join("vault.json"))?)?;
    Ok(metadata)
}

pub fn create_vault(
    path: &Path,
    passphrase: &SecretString,
    epoch_date: Option<NaiveDate>,
    nickname: &str,
) -> VaultResult<VaultMetadata> {
    secure_fs::ensure_private_dir(path)?;
    secure_fs::ensure_private_dir(&path.join("entries"))?;
    secure_fs::ensure_private_dir(&path.join("devices"))?;

    let created_date = Utc::now().date_naive();
    let epoch = epoch_date.unwrap_or(created_date);

    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let metadata = VaultMetadata {
        version: 1,
        created_at: Utc::now().to_rfc3339(),
        device_id: random_device_id(),
        kdf: KdfParams {
            algorithm: "argon2id".to_string(),
            memory_kib: DEFAULT_MEMORY_KIB,
            iterations: DEFAULT_ITERATIONS,
            parallelism: DEFAULT_PARALLELISM,
            salt_hex: hex::encode(salt),
        },
        options: VaultOptions {
            epoch_date: epoch.format("%Y-%m-%d").to_string(),
        },
    };

    let _ = derive_key(passphrase, &metadata.kdf)?;
    atomic_write(
        &path.join("vault.json"),
        &serde_json::to_vec_pretty(&metadata)?,
    )?;
    write_device_metadata(path, &metadata.device_id, nickname)?;

    Ok(metadata)
}

pub fn unlock_vault(path: &Path, passphrase: &SecretString) -> VaultResult<UnlockedVault> {
    let metadata = load_vault_metadata(path)?;
    let key = derive_key(passphrase, &metadata.kdf)?;
    Ok(UnlockedVault {
        root: path.to_path_buf(),
        device_id: metadata.device_id.clone(),
        metadata,
        key,
    })
}

pub fn unlock_vault_with_device(
    path: &Path,
    passphrase: &SecretString,
    device_id: impl Into<String>,
) -> VaultResult<UnlockedVault> {
    let mut unlocked = unlock_vault(path, passphrase)?;
    unlocked.device_id = device_id.into();
    Ok(unlocked)
}

pub fn register_device(path: &Path, device_id: &str, nickname: &str) -> VaultResult<()> {
    write_device_metadata(path, device_id, nickname)
}

pub fn random_device_id() -> String {
    let mut bytes = [0u8; 6];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn compute_entry_number(epoch: NaiveDate, entry_date: NaiveDate) -> String {
    let days = (entry_date - epoch).num_days() + 1;
    let value = if days < 1 { 0 } else { days as u64 };
    let width = 7usize.max(value.to_string().len());
    format!("{value:0width$}")
}

fn sanitize_entry_metadata(metadata: &EntryMetadata) -> EntryMetadata {
    fn normalize_list(values: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        for value in values {
            let normalized = value.trim().to_string();
            if normalized.is_empty() || out.contains(&normalized) {
                continue;
            }
            out.push(normalized);
        }
        out
    }

    EntryMetadata {
        tags: normalize_list(&metadata.tags),
        people: normalize_list(&metadata.people),
        project: normalize_optional_text(metadata.project.as_deref()),
        mood: metadata.mood.filter(|mood| *mood <= 9),
    }
}

fn metadata_search_text(
    body: &str,
    closing_thought: Option<&str>,
    metadata: &EntryMetadata,
) -> String {
    let mut lines = Vec::new();
    lines.push(body.to_string());
    if let Some(closing) = normalize_optional_text(closing_thought) {
        lines.push(format!("Closing Thought: {closing}"));
    }
    if !metadata.tags.is_empty() {
        lines.push(format!("Tags: {}", metadata.tags.join(", ")));
    }
    if !metadata.people.is_empty() {
        lines.push(format!("People: {}", metadata.people.join(", ")));
    }
    if let Some(project) = metadata.project.as_deref() {
        lines.push(format!("Project: {project}"));
    }
    if let Some(mood) = metadata.mood {
        lines.push(format!("Mood: {mood}"));
    }
    lines.join("\n")
}

fn search_cache_path(root: &Path) -> PathBuf {
    root.join(".cache").join("search-index.bsj.enc")
}

fn cache_aad_string() -> &'static str {
    "bsj:v1:search-cache"
}

fn build_catalog_fingerprint(root: &Path) -> VaultResult<Vec<CacheFingerprintEntry>> {
    let mut fingerprint = Vec::new();
    for relative in backup_source_paths(root)? {
        let relative_text = relative.to_string_lossy();
        if relative_text != "vault.json" && !relative_text.starts_with("entries/") {
            continue;
        }
        if relative_text != "vault.json" {
            let file_name = relative
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if !file_name.starts_with("rev-") || !file_name.ends_with(".bsj.enc") {
                continue;
            }
        }
        let full_path = root.join(&relative);
        let metadata = fs::metadata(&full_path)?;
        let modified_unix_secs = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        fingerprint.push(CacheFingerprintEntry {
            path: relative_text.to_string(),
            size: metadata.len(),
            modified_unix_secs,
        });
    }
    fingerprint.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(fingerprint)
}

fn top_counts<'a, I>(values: I) -> Vec<(String, usize)>
where
    I: Iterator<Item = &'a String>,
{
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for value in values {
        *counts.entry(value.to_ascii_lowercase()).or_insert(0) += 1;
    }
    let mut items = counts.into_iter().collect::<Vec<_>>();
    items.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    items.truncate(5);
    items
}

fn derive_key(passphrase: &SecretString, params: &KdfParams) -> VaultResult<Zeroizing<Vec<u8>>> {
    if !params.algorithm.eq_ignore_ascii_case("argon2id") {
        return Err(VaultError::InvalidFormat("unsupported KDF".to_string()));
    }
    let salt = hex::decode(&params.salt_hex)
        .map_err(|_| VaultError::InvalidFormat("invalid salt".to_string()))?;
    let argon2_params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| VaultError::InvalidFormat("invalid KDF parameters".to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);
    let mut key = Zeroizing::new(vec![0u8; KEY_LEN]);
    argon2
        .hash_password_into(passphrase.expose_secret().as_bytes(), &salt, &mut key)
        .map_err(|error| VaultError::Argon2(error.to_string()))?;
    Ok(key)
}

fn encrypt_payload(key: &[u8], plaintext: &[u8], aad: &[u8]) -> VaultResult<RevisionFile> {
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| VaultError::EncryptionFailed)?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| VaultError::EncryptionFailed)?;
    Ok(RevisionFile {
        version: FILE_VERSION,
        nonce,
        ciphertext,
    })
}

fn decrypt_payload(key: &[u8], encrypted: &RevisionFile, aad: &[u8]) -> VaultResult<Vec<u8>> {
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| VaultError::DecryptionFailed)?;
    cipher
        .decrypt(
            XNonce::from_slice(&encrypted.nonce),
            Payload {
                msg: &encrypted.ciphertext,
                aad,
            },
        )
        .map_err(|_| VaultError::DecryptionFailed)
}

fn aad_string(kind: &str, date: NaiveDate, device_id: &str, seq: u64) -> String {
    format!("bsj:v1:{kind}:{date}:{device_id}:{seq}")
}

fn parse_saved_at(input: &str) -> VaultResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(input)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| VaultError::InvalidFormat("invalid savedAt".to_string()))
}

fn date_dir(root: &Path, date: NaiveDate) -> PathBuf {
    root.join("entries")
        .join(format!("{:04}", date.year()))
        .join(date.format("%Y-%m-%d").to_string())
}

fn revision_file_name(device_id: &str, seq: u64) -> String {
    format!("rev-{device_id}-{seq:06}.bsj.enc")
}

fn draft_file_name(device_id: &str) -> String {
    format!("draft-{device_id}.bsj.enc")
}

fn parse_revision_name(file_name: &str) -> Option<(String, u64)> {
    let payload = file_name.strip_prefix("rev-")?.strip_suffix(".bsj.enc")?;
    let (device_id, seq_text) = payload.rsplit_once('-')?;
    let seq = seq_text.parse::<u64>().ok()?;
    Some((device_id.to_string(), seq))
}

fn list_revision_candidates(path: &Path) -> VaultResult<Vec<CandidateRevision>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Some((device_id, seq)) = parse_revision_name(&file_name) else {
            continue;
        };
        out.push(CandidateRevision {
            path: entry.path(),
            device_id,
            seq,
        });
    }
    Ok(out)
}

fn scan_date_revisions(
    root: &Path,
    key: &[u8],
    date: NaiveDate,
) -> VaultResult<Vec<RevisionRecord>> {
    let candidates = list_revision_candidates(&date_dir(root, date))?;
    let mut records = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let bytes = fs::read(&candidate.path)?;
        let revision_hash = hash_bytes_hex(&bytes);
        let encrypted = RevisionFile::parse(&bytes)?;
        let aad = aad_string("revision", date, &candidate.device_id, candidate.seq);
        let plaintext = decrypt_payload(key, &encrypted, aad.as_bytes())?;
        let payload: RevisionPayload = serde_json::from_slice(&plaintext)?;
        if payload.kind != "revision" {
            return Err(VaultError::InvalidFormat(
                "unexpected revision kind".to_string(),
            ));
        }
        if payload.device_id != candidate.device_id || payload.seq != candidate.seq {
            return Err(VaultError::InvalidFormat(
                "revision payload does not match filename".to_string(),
            ));
        }
        records.push(RevisionRecord {
            path: candidate.path,
            revision_hash,
            device_id: candidate.device_id,
            seq: candidate.seq,
            saved_at: parse_saved_at(&payload.saved_at)?,
            prev_hash: payload.prev_hash,
            merged_hashes: payload.merged_hashes,
            body: payload.body,
            closing_thought: payload.closing_thought,
            entry_metadata: payload.entry_metadata,
        });
    }
    records.sort_by(|left, right| compare_revision_record(right, left));
    Ok(records)
}

fn head_records(records: &[RevisionRecord]) -> Vec<RevisionRecord> {
    let mut referenced = HashSet::new();
    for record in records {
        if let Some(prev_hash) = &record.prev_hash {
            referenced.insert(prev_hash.clone());
        }
        referenced.extend(record.merged_hashes.iter().cloned());
    }

    let mut heads = records
        .iter()
        .filter(|record| !referenced.contains(&record.revision_hash))
        .cloned()
        .collect::<Vec<_>>();
    heads.sort_by(compare_revision_record);
    heads
}

fn compare_revision_record(left: &RevisionRecord, right: &RevisionRecord) -> Ordering {
    left.saved_at
        .cmp(&right.saved_at)
        .then_with(|| left.device_id.cmp(&right.device_id))
        .then_with(|| left.seq.cmp(&right.seq))
        .then_with(|| left.revision_hash.cmp(&right.revision_hash))
        .then_with(|| left.path.cmp(&right.path))
}

fn next_revision_sequence_for_device(records: &[RevisionRecord], device_id: &str) -> u64 {
    records
        .iter()
        .filter(|record| record.device_id == device_id)
        .map(|record| record.seq)
        .max()
        .unwrap_or(0)
        + 1
}

fn verify_records_for_date(date: NaiveDate, records: &[RevisionRecord]) -> Vec<IntegrityIssue> {
    let mut issues = Vec::new();
    let known_hashes = records
        .iter()
        .map(|record| record.revision_hash.clone())
        .collect::<HashSet<_>>();

    for record in records {
        if let Some(prev_hash) = &record.prev_hash
            && !known_hashes.contains(prev_hash)
        {
            issues.push(IntegrityIssue {
                date: Some(date),
                message: format!(
                    "missing previous revision {} for {}:{}",
                    prev_hash, record.device_id, record.seq
                ),
            });
        }
        for merged_hash in &record.merged_hashes {
            if !known_hashes.contains(merged_hash) {
                issues.push(IntegrityIssue {
                    date: Some(date),
                    message: format!(
                        "missing merged revision {} for {}:{}",
                        merged_hash, record.device_id, record.seq
                    ),
                });
            }
        }
    }

    if !records.is_empty() && head_records(records).is_empty() {
        issues.push(IntegrityIssue {
            date: Some(date),
            message: "revision graph has no head".to_string(),
        });
    }

    issues
}

fn write_device_metadata(root: &Path, device_id: &str, nickname: &str) -> VaultResult<()> {
    secure_fs::ensure_private_dir(&root.join("devices"))?;
    let device_metadata = DeviceMetadata {
        nickname: nickname.to_string(),
    };
    atomic_write(
        &root.join("devices").join(format!("{device_id}.json")),
        &serde_json::to_vec_pretty(&device_metadata)?,
    )
}

fn hash_bytes_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn format_export_text(body: &str, closing_thought: Option<&str>) -> String {
    match normalize_optional_text(closing_thought) {
        Some(closing_thought) if body.trim_end().is_empty() => closing_thought,
        Some(closing_thought) => format!("{}\n\n{closing_thought}", body.trim_end()),
        None => body.to_string(),
    }
}

fn normalize_optional_text(input: Option<&str>) -> Option<String> {
    input
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
}

fn entry_preview(body: &str, closing_thought: Option<&str>, max_chars: usize) -> String {
    let preview_source = if body.trim().is_empty() {
        closing_thought.unwrap_or_default()
    } else {
        body
    };
    first_line_preview(preview_source, max_chars)
}

fn first_line_preview(body: &str, max_chars: usize) -> String {
    let first_line = body.lines().next().unwrap_or_default().replace('\t', " ");
    truncate_chars(first_line.trim_end(), max_chars)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut truncated = input.chars().take(max_chars).collect::<String>();
    if input.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn build_backup_archive(root: &Path) -> VaultResult<Vec<u8>> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for relative in backup_source_paths(root)? {
            builder.append_path_with_name(root.join(&relative), &relative)?;
        }
        builder.finish()?;
    }
    Ok(tar_bytes)
}

fn unpack_backup_archive_safely(archive_bytes: &[u8], target_root: &Path) -> VaultResult<()> {
    ensure_safe_restore_root(target_root)?;
    let mut archive = tar::Archive::new(Cursor::new(archive_bytes));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let relative_path = sanitize_backup_member_path(entry.path()?.as_ref())?;
        let entry_type = entry.header().entry_type();

        if entry_type.is_symlink() || entry_type.is_hard_link() {
            return Err(VaultError::InvalidFormat(
                "backup archive contains unsupported link entry".to_string(),
            ));
        }

        let destination = resolve_restore_destination(target_root, &relative_path)?;
        if entry_type.is_dir() {
            ensure_safe_restore_directory(&destination)?;
            continue;
        }

        if !entry_type.is_file() {
            return Err(VaultError::InvalidFormat(
                "backup archive contains unsupported entry type".to_string(),
            ));
        }

        if let Some(parent) = destination.parent() {
            ensure_safe_restore_directory(parent)?;
        }
        reject_symlink_destination(&destination)?;

        let mut file_bytes = Vec::new();
        std::io::copy(&mut entry, &mut file_bytes)?;
        secure_fs::atomic_write_private(&destination, &file_bytes)?;
    }

    Ok(())
}

fn sanitize_backup_member_path(path: &Path) -> VaultResult<PathBuf> {
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(segment) => sanitized.push(segment),
            _ => {
                return Err(VaultError::InvalidFormat(
                    "backup archive contains invalid path".to_string(),
                ));
            }
        }
    }

    if sanitized.as_os_str().is_empty() {
        return Err(VaultError::InvalidFormat(
            "backup archive contains empty path".to_string(),
        ));
    }

    Ok(sanitized)
}

fn ensure_safe_restore_root(target_root: &Path) -> VaultResult<()> {
    match fs::symlink_metadata(target_root) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(VaultError::InvalidFormat(
                    "restore target root cannot be a symlink".to_string(),
                ));
            }
            if !file_type.is_dir() {
                return Err(VaultError::InvalidFormat(
                    "restore target root must be a directory".to_string(),
                ));
            }
            secure_fs::set_private_dir_permissions(target_root)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            secure_fs::ensure_private_dir(target_root)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn resolve_restore_destination(target_root: &Path, relative_path: &Path) -> VaultResult<PathBuf> {
    let mut current = target_root.to_path_buf();
    for component in relative_path.components() {
        match component {
            std::path::Component::Normal(segment) => current.push(segment),
            _ => {
                return Err(VaultError::InvalidFormat(
                    "backup archive contains invalid path".to_string(),
                ));
            }
        }
    }

    if let Some(parent) = current.parent() {
        ensure_safe_restore_directory(parent)?;
    }

    Ok(current)
}

fn ensure_safe_restore_directory(path: &Path) -> VaultResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                return Err(VaultError::InvalidFormat(
                    "restore target contains a symlinked directory".to_string(),
                ));
            }
            if !file_type.is_dir() {
                return Err(VaultError::InvalidFormat(
                    "restore target contains a non-directory path".to_string(),
                ));
            }
            secure_fs::set_private_dir_permissions(path)?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            secure_fs::ensure_private_dir(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn reject_symlink_destination(path: &Path) -> VaultResult<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(VaultError::InvalidFormat(
            "restore target contains a symlinked file".to_string(),
        )),
        Ok(metadata) if metadata.is_dir() => Err(VaultError::InvalidFormat(
            "restore target file path resolves to a directory".to_string(),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn backup_source_paths(root: &Path) -> VaultResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_backup_source_paths(root, root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_backup_source_paths(
    root: &Path,
    current: &Path,
    out: &mut Vec<PathBuf>,
) -> VaultResult<()> {
    if !current.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| VaultError::InvalidFormat("invalid backup source path".to_string()))?;

        if entry.file_type()?.is_dir() {
            if relative == Path::new("backups") {
                continue;
            }
            collect_backup_source_paths(root, &path, out)?;
            continue;
        }

        let relative = relative.to_path_buf();
        if should_include_in_backup(&relative) {
            out.push(relative);
        }
    }

    Ok(())
}

fn should_include_in_backup(relative: &Path) -> bool {
    let relative_text = relative.to_string_lossy();
    if relative_text == "vault.json" {
        return true;
    }
    if relative_text.starts_with("devices/") && relative_text.ends_with(".json") {
        return true;
    }
    if relative_text.starts_with("entries/") && relative_text.ends_with(".bsj.enc") {
        return true;
    }
    false
}

fn backup_file_name(created_at: DateTime<Utc>) -> String {
    format!("backup-{}.bsjbak.enc", created_at.format("%Y%m%dT%H%M%SZ"))
}

fn backup_aad_string(created_at: DateTime<Utc>) -> String {
    format!("bsj:v1:backup:{}", created_at.format("%Y%m%dT%H%M%SZ"))
}

fn parse_backup_timestamp(path: &Path) -> VaultResult<DateTime<Utc>> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| VaultError::InvalidFormat("invalid backup filename".to_string()))?;
    let timestamp = file_name
        .strip_prefix("backup-")
        .and_then(|value| value.strip_suffix(".bsjbak.enc"))
        .ok_or_else(|| VaultError::InvalidFormat("invalid backup filename".to_string()))?;
    chrono::NaiveDateTime::parse_from_str(timestamp, "%Y%m%dT%H%M%SZ")
        .map(|value| value.and_utc())
        .map_err(|_| VaultError::InvalidFormat("invalid backup timestamp".to_string()))
}

#[derive(Clone, Debug)]
struct BackupArtifact {
    path: PathBuf,
    created_at: DateTime<Utc>,
}

fn list_backup_artifacts(backup_dir: &Path) -> VaultResult<Vec<BackupArtifact>> {
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }
    let mut artifacts = Vec::new();
    for entry in fs::read_dir(backup_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(created_at) = parse_backup_timestamp(&path) else {
            continue;
        };
        artifacts.push(BackupArtifact { path, created_at });
    }
    artifacts.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(artifacts)
}

fn prune_backups(backup_dir: &Path, retention: &BackupRetentionConfig) -> VaultResult<usize> {
    let artifacts = prune_candidates(backup_dir, retention)?;
    let mut pruned = 0usize;
    for artifact in artifacts {
        fs::remove_file(artifact.path)?;
        pruned += 1;
    }
    Ok(pruned)
}

fn prune_candidates(
    backup_dir: &Path,
    retention: &BackupRetentionConfig,
) -> VaultResult<Vec<BackupArtifact>> {
    let artifacts = list_backup_artifacts(backup_dir)?;
    if artifacts.is_empty() {
        return Ok(Vec::new());
    }

    let mut keep = HashSet::new();
    if let Some(latest) = artifacts.first() {
        keep.insert(latest.path.clone());
    }

    let mut seen_days = HashSet::new();
    let mut seen_weeks = HashSet::new();
    let mut seen_months = HashSet::new();

    for artifact in &artifacts {
        if seen_days.len() < retention.daily && seen_days.insert(artifact.created_at.date_naive()) {
            keep.insert(artifact.path.clone());
        }

        let iso_week = artifact.created_at.iso_week();
        if seen_weeks.len() < retention.weekly
            && seen_weeks.insert((iso_week.year(), iso_week.week()))
        {
            keep.insert(artifact.path.clone());
        }

        let month_key = (artifact.created_at.year(), artifact.created_at.month());
        if seen_months.len() < retention.monthly && seen_months.insert(month_key) {
            keep.insert(artifact.path.clone());
        }
    }

    Ok(artifacts
        .into_iter()
        .filter(|artifact| !keep.contains(&artifact.path))
        .collect())
}

fn backup_artifact_to_entry(artifact: &BackupArtifact) -> VaultResult<BackupEntry> {
    Ok(BackupEntry {
        path: artifact.path.clone(),
        created_at: artifact.created_at,
        size_bytes: fs::metadata(&artifact.path)?.len(),
    })
}

fn atomic_write(path: &Path, bytes: &[u8]) -> VaultResult<()> {
    secure_fs::atomic_write_private(path, bytes).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackupRetentionConfig;
    use std::{thread::sleep, time::Duration};
    use tempfile::tempdir;

    fn build_raw_tar_archive(
        path: &str,
        entry_type: u8,
        payload: &[u8],
        link_name: &str,
    ) -> Vec<u8> {
        let mut header = [0u8; 512];
        copy_tar_field(&mut header[0..100], path.as_bytes());
        write_tar_octal(&mut header[100..108], 0o600);
        write_tar_octal(&mut header[108..116], 0);
        write_tar_octal(&mut header[116..124], 0);
        write_tar_octal(&mut header[124..136], payload.len() as u64);
        write_tar_octal(&mut header[136..148], 0);
        header[148..156].fill(b' ');
        header[156] = entry_type;
        copy_tar_field(&mut header[157..257], link_name.as_bytes());
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");

        let checksum: u32 = header.iter().map(|byte| *byte as u32).sum();
        write_tar_checksum(&mut header[148..156], checksum);

        let mut archive = Vec::from(header);
        archive.extend_from_slice(payload);
        let padding = (512 - (payload.len() % 512)) % 512;
        archive.extend(std::iter::repeat_n(0u8, padding));
        archive.extend([0u8; 1024]);
        archive
    }

    fn copy_tar_field(field: &mut [u8], bytes: &[u8]) {
        assert!(
            bytes.len() <= field.len(),
            "tar field overflow in test fixture"
        );
        field[..bytes.len()].copy_from_slice(bytes);
    }

    fn write_tar_octal(field: &mut [u8], value: u64) {
        let width = field.len() - 1;
        let encoded = format!("{value:0width$o}", width = width);
        assert!(
            encoded.len() <= width,
            "octal value too large for tar field in test fixture"
        );
        field[..width].copy_from_slice(encoded.as_bytes());
        field[width] = 0;
    }

    fn write_tar_checksum(field: &mut [u8], checksum: u32) {
        let encoded = format!("{checksum:06o}");
        field[..6].copy_from_slice(encoded.as_bytes());
        field[6] = 0;
        field[7] = b' ';
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = vec![9u8; KEY_LEN];
        let aad = b"bsj-test";
        let body = b"hello secret";
        let encrypted = encrypt_payload(&key, body, aad).expect("encrypt");
        let decrypted = decrypt_payload(&key, &encrypted, aad).expect("decrypt");
        assert_eq!(decrypted, body);
    }

    #[test]
    fn revision_ordering_uses_latest_saved_revision() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock vault");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault.save_revision(date, "first").expect("save first");
        sleep(Duration::from_millis(10));
        vault.save_revision(date, "second").expect("save second");

        let loaded = vault.load_date_state(date).expect("load date");
        assert_eq!(loaded.revision_text.as_deref(), Some("second"));
        assert!(loaded.recovery_draft_text.is_none());
        assert!(loaded.conflict.is_none());
    }

    #[test]
    fn draft_newer_detection_prefers_draft() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock vault");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault
            .save_revision(date, "saved revision")
            .expect("save revision");
        sleep(Duration::from_millis(10));
        vault.save_draft(date, "draft body").expect("save draft");

        let loaded = vault.load_date_state(date).expect("load date");
        assert_eq!(loaded.revision_text.as_deref(), Some("saved revision"));
        assert_eq!(loaded.recovery_draft_text.as_deref(), Some("draft body"));
    }

    #[test]
    fn encrypted_files_do_not_contain_plaintext() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock vault");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let body = "needle text 42";

        vault.save_revision(date, body).expect("save revision");
        vault.save_draft(date, body).expect("save draft");

        let day_dir = date_dir(&root, date);
        let mut file_bytes = Vec::new();
        for entry in fs::read_dir(day_dir).expect("read day dir") {
            let entry = entry.expect("entry");
            if entry.file_type().expect("file type").is_file() {
                file_bytes.extend(fs::read(entry.path()).expect("read file"));
            }
        }
        let blob = String::from_utf8_lossy(&file_bytes);
        assert!(!blob.contains(body));
    }

    #[test]
    fn entry_number_edge_cases() {
        let epoch = NaiveDate::from_ymd_opt(2026, 3, 16).expect("epoch");
        assert_eq!(compute_entry_number(epoch, epoch), "0000001");
        assert_eq!(
            compute_entry_number(epoch, NaiveDate::from_ymd_opt(2026, 3, 17).expect("date")),
            "0000002"
        );
        assert_eq!(
            compute_entry_number(epoch, NaiveDate::from_ymd_opt(2026, 3, 15).expect("date")),
            "0000000"
        );
        assert_eq!(
            compute_entry_number(
                epoch,
                epoch
                    .checked_add_signed(chrono::Duration::days(4_772))
                    .expect("shift")
            ),
            "0004773"
        );
    }

    #[test]
    fn list_entry_dates_scans_and_sorts_saved_dates() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock vault");

        let early = NaiveDate::from_ymd_opt(2026, 3, 14).expect("date");
        let late = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        vault.save_revision(late, "late").expect("save late");
        vault.save_revision(early, "early").expect("save early");
        vault
            .save_draft(
                NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"),
                "draft only",
            )
            .expect("save draft");

        assert_eq!(
            vault.list_entry_dates().expect("list dates"),
            vec![early, late]
        );
    }

    #[test]
    fn index_entries_use_latest_revision_preview_only() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(
            &root,
            &passphrase,
            Some(NaiveDate::from_ymd_opt(2026, 3, 1).expect("date")),
            "Test",
        )
        .expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock vault");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault
            .save_revision(date, "first line\nsecond line")
            .expect("save first");
        sleep(Duration::from_millis(10));
        vault
            .save_revision(date, "updated preview\nbody")
            .expect("save second");

        let entries = vault.list_index_entries(12).expect("index entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].date, date);
        assert_eq!(entries[0].entry_number, "0000016");
        assert_eq!(entries[0].preview, "updated prev...");
        assert!(!entries[0].has_conflict);
    }

    #[test]
    fn conflicting_heads_are_detected_after_divergent_saves() {
        let dir = tempdir().expect("tempdir");
        let source_root = dir.path().join("source");
        let clone_root = dir.path().join("clone");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&source_root, &passphrase, None, "Test").expect("create vault");
        fs::create_dir_all(&clone_root).expect("clone dir");
        copy_dir_recursive(&source_root, &clone_root).expect("clone vault");

        let vault_a =
            unlock_vault_with_device(&source_root, &passphrase, "device-a").expect("unlock a");
        let _vault_b =
            unlock_vault_with_device(&clone_root, &passphrase, "device-b").expect("unlock b");
        register_device(&source_root, "device-a", "A").expect("register a");
        register_device(&clone_root, "device-b", "B").expect("register b");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault_a.save_revision(date, "shared").expect("save shared");
        copy_dir_recursive(&source_root, &clone_root).expect("sync base to clone");
        let vault_a =
            unlock_vault_with_device(&source_root, &passphrase, "device-a").expect("unlock a");
        let vault_b =
            unlock_vault_with_device(&clone_root, &passphrase, "device-b").expect("unlock b");
        sleep(Duration::from_millis(10));
        vault_a
            .save_revision(date, "branch a")
            .expect("save branch a");
        sleep(Duration::from_millis(10));
        vault_b
            .save_revision(date, "branch b")
            .expect("save branch b");

        copy_dir_recursive(&clone_root, &source_root).expect("manual sync");
        let synced =
            unlock_vault_with_device(&source_root, &passphrase, "device-a").expect("unlock synced");
        let loaded = synced.load_date_state(date).expect("load state");
        let conflict = loaded.conflict.expect("conflict");
        assert_eq!(conflict.heads.len(), 2);
        assert_eq!(
            synced.list_conflicted_dates().expect("conflicts"),
            vec![date]
        );
    }

    #[test]
    fn merge_revision_resolves_conflict_without_deleting_branches() {
        let dir = tempdir().expect("tempdir");
        let source_root = dir.path().join("source");
        let clone_root = dir.path().join("clone");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&source_root, &passphrase, None, "Test").expect("create vault");
        fs::create_dir_all(&clone_root).expect("clone dir");
        copy_dir_recursive(&source_root, &clone_root).expect("clone vault");

        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        let vault_a =
            unlock_vault_with_device(&source_root, &passphrase, "device-a").expect("unlock a");
        vault_a.save_revision(date, "shared").expect("save shared");
        copy_dir_recursive(&source_root, &clone_root).expect("sync base");

        let vault_a =
            unlock_vault_with_device(&source_root, &passphrase, "device-a").expect("unlock a");
        let vault_b =
            unlock_vault_with_device(&clone_root, &passphrase, "device-b").expect("unlock b");
        vault_a
            .save_revision(date, "branch a")
            .expect("save branch a");
        sleep(Duration::from_millis(10));
        vault_b
            .save_revision(date, "branch b")
            .expect("save branch b");

        let sync_report = vault_a.sync_folder(&clone_root).expect("sync");
        assert_eq!(sync_report.conflicts, vec![date]);

        let conflicted =
            unlock_vault_with_device(&source_root, &passphrase, "device-a").expect("unlock merged");
        let conflict = conflicted
            .load_date_state(date)
            .expect("load state")
            .conflict
            .expect("conflict");
        let primary = conflict.heads.first().expect("primary");
        let merged_hashes = conflict
            .heads
            .iter()
            .skip(1)
            .map(|head| head.revision_hash.clone())
            .collect::<Vec<_>>();

        conflicted
            .save_merge_revision(date, "merged body", &primary.revision_hash, &merged_hashes)
            .expect("save merge");

        let resolved = conflicted.load_date_state(date).expect("resolved state");
        assert_eq!(resolved.revision_text.as_deref(), Some("merged body"));
        assert!(resolved.conflict.is_none());
        assert_eq!(
            list_revision_candidates(&date_dir(&source_root, date))
                .expect("revisions")
                .len(),
            4
        );
    }

    #[test]
    fn sync_reconciles_missing_revisions_between_folders() {
        let dir = tempdir().expect("tempdir");
        let local_root = dir.path().join("local");
        let remote_root = dir.path().join("remote");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&local_root, &passphrase, None, "Local").expect("create local");
        copy_dir_recursive(&local_root, &remote_root).expect("seed remote");
        register_device(&remote_root, "device-b", "Remote").expect("register remote device");

        let local =
            unlock_vault_with_device(&local_root, &passphrase, "device-a").expect("unlock local");
        let remote =
            unlock_vault_with_device(&remote_root, &passphrase, "device-b").expect("unlock remote");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        local
            .save_revision(date, "local entry")
            .expect("save local");
        remote
            .save_revision(
                NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"),
                "remote entry",
            )
            .expect("save remote");

        let report = local.sync_folder(&remote_root).expect("sync");
        assert_eq!(report.pulled, 1);
        assert_eq!(report.pushed, 1);
        assert!(report.conflicts.is_empty());

        let local_after =
            unlock_vault_with_device(&local_root, &passphrase, "device-a").expect("unlock after");
        let dates = local_after.list_entry_dates().expect("dates");
        assert_eq!(dates.len(), 2);
        assert!(dates.contains(&date));
        assert!(dates.contains(&NaiveDate::from_ymd_opt(2026, 3, 17).expect("date")));
    }

    #[test]
    fn verify_reports_missing_hashchain_predecessor() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault_with_device(&root, &passphrase, "device-a").expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault.save_revision(date, "first").expect("save first");
        sleep(Duration::from_millis(10));
        vault.save_revision(date, "second").expect("save second");

        let revisions = list_revision_candidates(&date_dir(&root, date)).expect("revs");
        let first_path = revisions
            .into_iter()
            .find(|candidate| candidate.seq == 1)
            .expect("first rev")
            .path;
        fs::remove_file(first_path).expect("remove first");

        let report = vault.verify_integrity().expect("verify");
        assert!(!report.ok);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("missing previous revision"))
        );
    }

    #[test]
    fn verify_reports_tampered_revision_file() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault_with_device(&root, &passphrase, "device-a").expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault.save_revision(date, "first").expect("save first");
        let revision_path = list_revision_candidates(&date_dir(&root, date))
            .expect("revisions")
            .into_iter()
            .next()
            .expect("revision")
            .path;

        let mut bytes = fs::read(&revision_path).expect("read revision");
        let last = bytes.len().saturating_sub(1);
        bytes[last] ^= 0x55;
        fs::write(&revision_path, bytes).expect("write revision");

        let report = vault.verify_integrity().expect("verify");
        assert!(!report.ok);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.message.contains("decryption failed"))
        );
    }

    #[test]
    fn closing_thought_persists_and_exports_as_final_line() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault
            .save_entry_revision(
                date,
                "Body text",
                Some("Good night."),
                &EntryMetadata::default(),
            )
            .expect("save");

        let state = vault.load_date_state(date).expect("load");
        assert_eq!(
            state.revision_closing_thought.as_deref(),
            Some("Good night.")
        );
        assert_eq!(
            vault.export_entry_text(date).expect("export"),
            Some("Body text\n\nGood night.".to_string())
        );
    }

    #[test]
    fn backup_roundtrip_restores_encrypted_snapshot() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let restore_root = dir.path().join("restored");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault
            .save_entry_revision(
                date,
                "secret backup body",
                Some("Lights out."),
                &EntryMetadata::default(),
            )
            .expect("save revision");
        vault
            .save_entry_draft(
                date,
                "secret draft body",
                Some("Draft closing."),
                &EntryMetadata::default(),
            )
            .expect("save draft");

        let backup = vault
            .create_backup(&BackupRetentionConfig::default())
            .expect("backup");
        let backup_bytes = fs::read(&backup.path).expect("read backup");
        let backup_blob = String::from_utf8_lossy(&backup_bytes);
        assert!(!backup_blob.contains("secret backup body"));
        assert!(!backup_blob.contains("secret draft body"));

        vault
            .restore_backup_into(&backup.path, &restore_root)
            .expect("restore");

        let restored = unlock_vault(&restore_root, &passphrase).expect("unlock restored");
        let restored_state = restored.load_date_state(date).expect("load restored");
        assert_eq!(
            restored_state.revision_text.as_deref(),
            Some("secret backup body")
        );
        assert_eq!(
            restored_state.revision_closing_thought.as_deref(),
            Some("Lights out.")
        );
        assert_eq!(
            restored_state.recovery_draft_text.as_deref(),
            Some("secret draft body")
        );
        assert_eq!(
            restored_state.recovery_draft_closing_thought.as_deref(),
            Some("Draft closing.")
        );
    }

    #[test]
    fn restore_rejects_backup_path_traversal_entries() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let restore_root = dir.path().join("restored");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");

        let timestamp = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let backup_dir = root.join("backups");
        secure_fs::ensure_private_dir(&backup_dir).expect("backup dir");
        let backup_path = backup_dir.join(backup_file_name(timestamp));

        let tar_bytes = build_raw_tar_archive("../escape.txt", b'0', b"owned", "");

        let compressed = zstd::stream::encode_all(Cursor::new(tar_bytes), 3).expect("compress");
        let encrypted = encrypt_payload(
            &vault.key,
            &compressed,
            backup_aad_string(timestamp).as_bytes(),
        )
        .expect("encrypt");
        atomic_write(&backup_path, &encrypted.to_bytes()).expect("write backup");

        let error = vault
            .restore_backup_into(&backup_path, &restore_root)
            .expect_err("reject traversal");
        assert!(error.to_string().contains("invalid path"));
        assert!(!dir.path().join("escape.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn restore_rejects_symlinked_target_directories() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let restore_root = dir.path().join("restored");
        let outside = dir.path().join("outside");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");

        let timestamp = DateTime::parse_from_rfc3339("2026-03-17T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let backup_dir = root.join("backups");
        secure_fs::ensure_private_dir(&backup_dir).expect("backup dir");
        let backup_path = backup_dir.join(backup_file_name(timestamp));

        secure_fs::ensure_private_dir(&outside).expect("outside dir");
        secure_fs::ensure_private_dir(&restore_root).expect("restore root");
        symlink(&outside, restore_root.join("entries")).expect("symlink");

        let tar_bytes = build_raw_tar_archive("entries/2026-03-17.txt", b'0', b"owned", "");
        let compressed = zstd::stream::encode_all(Cursor::new(tar_bytes), 3).expect("compress");
        let encrypted = encrypt_payload(
            &vault.key,
            &compressed,
            backup_aad_string(timestamp).as_bytes(),
        )
        .expect("encrypt");
        atomic_write(&backup_path, &encrypted.to_bytes()).expect("write backup");

        let error = vault
            .restore_backup_into(&backup_path, &restore_root)
            .expect_err("reject symlinked target");
        assert!(error.to_string().contains("symlinked directory"));
        assert!(!outside.join("2026-03-17.txt").exists());
    }

    #[test]
    fn backup_listing_and_prune_preview_work() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        vault
            .save_entry_revision(
                date,
                "secret backup body",
                Some("Lights out."),
                &EntryMetadata::default(),
            )
            .expect("save revision");

        let created = vault
            .create_backup(&BackupRetentionConfig {
                daily: 7,
                weekly: 4,
                monthly: 6,
            })
            .expect("backup");
        let original_bytes = fs::read(&created.path).expect("read backup");
        let backup_dir = root.join("backups");
        fs::write(
            backup_dir.join("backup-20260316T000000Z.bsjbak.enc"),
            &original_bytes,
        )
        .expect("seed old backup");
        fs::write(
            backup_dir.join("backup-20260317T000000Z.bsjbak.enc"),
            &original_bytes,
        )
        .expect("seed new backup");

        let backups = vault.list_backups().expect("list backups");
        assert!(backups.len() >= 2);

        let prune_preview = vault
            .preview_backup_prune(&BackupRetentionConfig {
                daily: 1,
                weekly: 0,
                monthly: 0,
            })
            .expect("prune preview");
        assert!(!prune_preview.is_empty());
    }

    #[test]
    fn export_entry_includes_entry_number() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        let epoch = NaiveDate::from_ymd_opt(2026, 3, 1).expect("epoch");
        create_vault(&root, &passphrase, Some(epoch), "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");

        vault
            .save_entry_revision(
                date,
                "body text",
                Some("Good night."),
                &EntryMetadata::default(),
            )
            .expect("save revision");

        let exported = vault.export_entry(date).expect("export").expect("entry");
        assert_eq!(exported.entry_number, compute_entry_number(epoch, date));
        assert_eq!(exported.body, "body text");
        assert_eq!(exported.closing_thought.as_deref(), Some("Good night."));
    }

    fn copy_dir_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let path = entry.path();
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir_recursive(&path, &target)?;
            } else {
                fs::create_dir_all(target.parent().expect("parent"))?;
                fs::copy(path, target)?;
            }
        }
        Ok(())
    }
}
