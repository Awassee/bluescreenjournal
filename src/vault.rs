use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rand::{RngCore, rngs::OsRng};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    fs,
    io::Write,
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

#[derive(Clone, Debug)]
pub struct LoadedDateState {
    pub revision_text: Option<String>,
    pub recovery_draft_text: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub date: NaiveDate,
    pub entry_number: String,
    pub preview: String,
    pub has_conflict: bool,
}

#[derive(Clone, Debug)]
struct RevisionInfo {
    path: PathBuf,
    device_id: String,
    seq: u64,
    saved_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct DraftInfo {
    body: String,
    saved_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct CandidateRevision {
    path: PathBuf,
    device_id: String,
    seq: u64,
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
}

pub struct UnlockedVault {
    root: PathBuf,
    metadata: VaultMetadata,
    key: Zeroizing<Vec<u8>>,
}

impl UnlockedVault {
    pub fn metadata(&self) -> &VaultMetadata {
        &self.metadata
    }

    pub fn save_revision(&self, date: NaiveDate, body: &str) -> VaultResult<()> {
        let date_directory = date_dir(&self.root, date);
        fs::create_dir_all(&date_directory)?;
        let seq = self.next_revision_sequence(date)?;
        let payload = RevisionPayload {
            kind: "revision".to_string(),
            saved_at: Utc::now().to_rfc3339(),
            date: date.format("%Y-%m-%d").to_string(),
            body: body.to_string(),
            seq,
            device_id: self.metadata.device_id.clone(),
        };
        let plaintext = serde_json::to_vec(&payload)?;
        let aad = aad_string("revision", date, &self.metadata.device_id, seq);
        let encrypted = encrypt_payload(&self.key, &plaintext, aad.as_bytes())?;
        let path = date_directory.join(revision_file_name(&self.metadata.device_id, seq));
        atomic_write(&path, &encrypted.to_bytes())
    }

    pub fn save_draft(&self, date: NaiveDate, body: &str) -> VaultResult<()> {
        let date_directory = date_dir(&self.root, date);
        fs::create_dir_all(&date_directory)?;
        let payload = DraftPayload {
            kind: "draft".to_string(),
            saved_at: Utc::now().to_rfc3339(),
            date: date.format("%Y-%m-%d").to_string(),
            body: body.to_string(),
            device_id: self.metadata.device_id.clone(),
        };
        let plaintext = serde_json::to_vec(&payload)?;
        let aad = aad_string("draft", date, &self.metadata.device_id, 0);
        let encrypted = encrypt_payload(&self.key, &plaintext, aad.as_bytes())?;
        let path = date_directory.join(draft_file_name(&self.metadata.device_id));
        atomic_write(&path, &encrypted.to_bytes())
    }

    pub fn load_date_state(&self, date: NaiveDate) -> VaultResult<LoadedDateState> {
        let latest_revision = self.latest_revision_info(date)?;
        let revision_text = if let Some(revision) = &latest_revision {
            Some(self.read_revision_body(date, revision)?)
        } else {
            None
        };

        let draft = self.read_draft(date)?;
        let recovery_draft_text = match (draft, latest_revision) {
            (Some(draft), Some(revision)) => {
                if draft.saved_at > revision.saved_at {
                    Some(draft.body)
                } else {
                    None
                }
            }
            (Some(draft), None) => Some(draft.body),
            _ => None,
        };

        Ok(LoadedDateState {
            revision_text,
            recovery_draft_text,
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

    pub fn list_index_entries(&self, preview_chars: usize) -> VaultResult<Vec<IndexEntry>> {
        let epoch = self.metadata.epoch_date()?;
        let mut dates = self.list_entry_dates()?;
        dates.sort_unstable_by(|left, right| right.cmp(left));

        let mut entries = Vec::with_capacity(dates.len());
        for date in dates {
            let Some(revision) = self.latest_revision_info(date)? else {
                continue;
            };
            let preview = self.read_revision_preview(date, &revision, preview_chars)?;
            entries.push(IndexEntry {
                date,
                entry_number: compute_entry_number(epoch, date),
                preview,
                has_conflict: false,
            });
        }

        Ok(entries)
    }

    fn latest_revision_info(&self, date: NaiveDate) -> VaultResult<Option<RevisionInfo>> {
        let candidates = list_revision_candidates(&date_dir(&self.root, date))?;
        let Some(candidate) = candidates.into_iter().max_by(compare_revision_candidate) else {
            return Ok(None);
        };
        let payload = self.read_revision_payload(date, &candidate)?;
        Ok(Some(RevisionInfo {
            path: candidate.path,
            device_id: candidate.device_id,
            seq: candidate.seq,
            saved_at: parse_saved_at(&payload.saved_at)?,
        }))
    }

    fn read_revision_body(&self, date: NaiveDate, revision: &RevisionInfo) -> VaultResult<String> {
        let payload = self.read_revision_payload(
            date,
            &CandidateRevision {
                path: revision.path.clone(),
                device_id: revision.device_id.clone(),
                seq: revision.seq,
            },
        )?;
        Ok(payload.body)
    }

    fn read_revision_preview(
        &self,
        date: NaiveDate,
        revision: &RevisionInfo,
        preview_chars: usize,
    ) -> VaultResult<String> {
        let body = self.read_revision_body(date, revision)?;
        Ok(first_line_preview(&body, preview_chars))
    }

    fn read_revision_payload(
        &self,
        date: NaiveDate,
        revision: &CandidateRevision,
    ) -> VaultResult<RevisionPayload> {
        let bytes = fs::read(&revision.path)?;
        let encrypted = RevisionFile::parse(&bytes)?;
        let aad = aad_string("revision", date, &revision.device_id, revision.seq);
        let plaintext = decrypt_payload(&self.key, &encrypted, aad.as_bytes())?;
        serde_json::from_slice(&plaintext).map_err(Into::into)
    }

    fn read_draft(&self, date: NaiveDate) -> VaultResult<Option<DraftInfo>> {
        let path = date_dir(&self.root, date).join(draft_file_name(&self.metadata.device_id));
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        let encrypted = RevisionFile::parse(&bytes)?;
        let aad = aad_string("draft", date, &self.metadata.device_id, 0);
        let plaintext = decrypt_payload(&self.key, &encrypted, aad.as_bytes())?;
        let payload: DraftPayload = serde_json::from_slice(&plaintext)?;
        Ok(Some(DraftInfo {
            body: payload.body,
            saved_at: parse_saved_at(&payload.saved_at)?,
        }))
    }

    fn next_revision_sequence(&self, date: NaiveDate) -> VaultResult<u64> {
        let candidates = list_revision_candidates(&date_dir(&self.root, date))?;
        Ok(candidates
            .into_iter()
            .filter(|candidate| candidate.device_id == self.metadata.device_id)
            .map(|candidate| candidate.seq)
            .max()
            .unwrap_or(0)
            + 1)
    }
}

pub fn vault_exists(path: &Path) -> bool {
    path.join("vault.json").is_file()
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
    let device_metadata = DeviceMetadata {
        nickname: nickname.to_string(),
    };
    atomic_write(
        &path
            .join("devices")
            .join(format!("{}.json", metadata.device_id.clone())),
        &serde_json::to_vec_pretty(&device_metadata)?,
    )?;

    Ok(metadata)
}

pub fn unlock_vault(path: &Path, passphrase: &SecretString) -> VaultResult<UnlockedVault> {
    let metadata: VaultMetadata = serde_json::from_slice(&fs::read(path.join("vault.json"))?)?;
    let key = derive_key(passphrase, &metadata.kdf)?;
    Ok(UnlockedVault {
        root: path.to_path_buf(),
        metadata,
        key,
    })
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

fn compare_revision_candidate(left: &CandidateRevision, right: &CandidateRevision) -> Ordering {
    left.seq
        .cmp(&right.seq)
        .then_with(|| left.device_id.cmp(&right.device_id))
        .then_with(|| left.path.cmp(&right.path))
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

fn random_device_id() -> String {
    let mut bytes = [0u8; 6];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
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
}
