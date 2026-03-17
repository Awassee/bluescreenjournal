use crate::{
    config::BackupRetentionConfig,
    search::SearchDocument,
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
    io::{Cursor, Write},
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
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictState {
    pub date: NaiveDate,
    pub heads: Vec<ConflictHead>,
}

#[derive(Clone, Debug)]
pub struct LoadedDateState {
    pub revision_text: Option<String>,
    pub revision_closing_thought: Option<String>,
    pub recovery_draft_text: Option<String>,
    pub recovery_draft_closing_thought: Option<String>,
    pub conflict: Option<ConflictState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub date: NaiveDate,
    pub entry_number: String,
    pub preview: String,
    pub has_conflict: bool,
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

#[derive(Clone, Debug)]
struct DraftInfo {
    body: String,
    closing_thought: Option<String>,
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
        self.save_entry_revision(date, body, None)
    }

    pub fn save_entry_revision(
        &self,
        date: NaiveDate,
        body: &str,
        closing_thought: Option<&str>,
    ) -> VaultResult<()> {
        let records = self.scan_date_revisions(date)?;
        let heads = head_records(&records);
        let prev_hash = heads.first().map(|record| record.revision_hash.clone());
        self.save_revision_internal(date, body, closing_thought, prev_hash, Vec::new(), &records)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_merge_revision(
        &self,
        date: NaiveDate,
        body: &str,
        primary_hash: &str,
        merged_hashes: &[String],
    ) -> VaultResult<()> {
        self.save_entry_merge_revision(date, body, None, primary_hash, merged_hashes)
    }

    pub fn save_entry_merge_revision(
        &self,
        date: NaiveDate,
        body: &str,
        closing_thought: Option<&str>,
        primary_hash: &str,
        merged_hashes: &[String],
    ) -> VaultResult<()> {
        let records = self.scan_date_revisions(date)?;
        self.save_revision_internal(
            date,
            body,
            closing_thought,
            Some(primary_hash.to_string()),
            merged_hashes.to_vec(),
            &records,
        )
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save_draft(&self, date: NaiveDate, body: &str) -> VaultResult<()> {
        self.save_entry_draft(date, body, None)
    }

    pub fn save_entry_draft(
        &self,
        date: NaiveDate,
        body: &str,
        closing_thought: Option<&str>,
    ) -> VaultResult<()> {
        let date_directory = date_dir(&self.root, date);
        fs::create_dir_all(&date_directory)?;
        let payload = DraftPayload {
            kind: "draft".to_string(),
            saved_at: Utc::now().to_rfc3339(),
            date: date.format("%Y-%m-%d").to_string(),
            body: body.to_string(),
            device_id: self.device_id.clone(),
            closing_thought: normalize_optional_text(closing_thought),
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
        fs::create_dir_all(&backup_dir)?;
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
        fs::create_dir_all(target_root)?;
        let mut archive = tar::Archive::new(Cursor::new(archive_bytes));
        archive.unpack(target_root)?;
        Ok(())
    }

    pub fn load_date_state(&self, date: NaiveDate) -> VaultResult<LoadedDateState> {
        let records = self.scan_date_revisions(date)?;
        let heads = head_records(&records);
        let primary = heads.first().or_else(|| records.first());
        let revision_text = primary.map(|record| record.body.clone());
        let revision_closing_thought = primary.and_then(|record| record.closing_thought.clone());

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
                        preview: entry_preview(&record.body, record.closing_thought.as_deref(), 54),
                    })
                    .collect(),
            })
        } else {
            None
        };

        let draft = self.read_draft(date)?;
        let (recovery_draft_text, recovery_draft_closing_thought) = match (draft, primary) {
            (Some(draft), Some(revision)) if draft.saved_at > revision.saved_at => {
                (Some(draft.body), draft.closing_thought)
            }
            (Some(draft), None) => (Some(draft.body), draft.closing_thought),
            _ => (None, None),
        };

        Ok(LoadedDateState {
            revision_text,
            revision_closing_thought,
            recovery_draft_text,
            recovery_draft_closing_thought,
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
        let epoch = self.metadata.epoch_date()?;
        let mut dates = self.list_entry_dates()?;
        dates.sort_unstable_by(|left, right| right.cmp(left));

        let mut entries = Vec::with_capacity(dates.len());
        for date in dates {
            let records = self.scan_date_revisions(date)?;
            let heads = head_records(&records);
            let primary = heads.first().or_else(|| records.first());
            if let Some(record) = primary {
                entries.push(IndexEntry {
                    date,
                    entry_number: compute_entry_number(epoch, date),
                    preview: entry_preview(
                        record.body.as_str(),
                        record.closing_thought.as_deref(),
                        preview_chars,
                    ),
                    has_conflict: heads.len() > 1,
                });
            }
        }

        Ok(entries)
    }

    pub fn load_search_documents(&self) -> VaultResult<Vec<SearchDocument>> {
        let epoch = self.metadata.epoch_date()?;
        let mut dates = self.list_entry_dates()?;
        dates.sort_unstable_by(|left, right| right.cmp(left));

        let mut documents = Vec::with_capacity(dates.len());
        for date in dates {
            let records = self.scan_date_revisions(date)?;
            let heads = head_records(&records);
            let primary = heads.first().or_else(|| records.first());
            if let Some(record) = primary {
                documents.push(SearchDocument {
                    date,
                    entry_number: compute_entry_number(epoch, date),
                    body: record.body.clone(),
                });
            }
        }

        Ok(documents)
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
        body: &str,
        closing_thought: Option<&str>,
        prev_hash: Option<String>,
        mut merged_hashes: Vec<String>,
        existing_records: &[RevisionRecord],
    ) -> VaultResult<()> {
        let date_directory = date_dir(&self.root, date);
        fs::create_dir_all(&date_directory)?;

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
    fs::create_dir_all(path)?;
    fs::create_dir_all(path.join("entries"))?;
    fs::create_dir_all(path.join("devices"))?;

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
    let parent = path
        .parent()
        .ok_or_else(|| VaultError::InvalidFormat("missing parent directory".to_string()))?;
    fs::create_dir_all(parent)?;

    let mut suffix = [0u8; 4];
    OsRng.fill_bytes(&mut suffix);
    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tmp"),
        hex::encode(suffix)
    ));

    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&tmp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackupRetentionConfig;
    use std::{thread::sleep, time::Duration};
    use tempfile::tempdir;

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
            .save_entry_revision(date, "Body text", Some("Good night."))
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
            .save_entry_revision(date, "secret backup body", Some("Lights out."))
            .expect("save revision");
        vault
            .save_entry_draft(date, "secret draft body", Some("Draft closing."))
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
    fn backup_listing_and_prune_preview_work() {
        let dir = tempdir().expect("tempdir");
        let root = dir.path().join("vault");
        let passphrase = SecretString::new("correct horse battery staple".into());
        create_vault(&root, &passphrase, None, "Test").expect("create vault");
        let vault = unlock_vault(&root, &passphrase).expect("unlock");
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).expect("date");
        vault
            .save_entry_revision(date, "secret backup body", Some("Lights out."))
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
            .save_entry_revision(date, "body text", Some("Good night."))
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
