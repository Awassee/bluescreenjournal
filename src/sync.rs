use crate::vault::{VaultError, VaultMetadata, VaultResult};
use aws_config::BehaviorVersion;
use aws_sdk_s3::{Client as S3Client, primitives::ByteStream};
use chrono::NaiveDate;
use rand::{RngCore, rngs::OsRng};
use reqwest::{
    Method, StatusCode, Url,
    blocking::{Client as HttpClient, RequestBuilder},
};
use roxmltree::Document;
use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendSyncReport {
    pub pulled: usize,
    pub pushed: usize,
}

pub trait SyncBackend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>>;
    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>>;
    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SyncObjectKind {
    Revision,
    Metadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SyncInventory {
    keys: BTreeSet<String>,
    revision_keys: BTreeSet<String>,
}

pub struct FolderBackend {
    root: PathBuf,
}

impl FolderBackend {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl SyncBackend for FolderBackend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
        Ok(list_local_inventory(&self.root)?.keys)
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        fs::read(sync_key_to_path(&self.root, key)?).map_err(Into::into)
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        write_local_key(&self.root, key, bytes)
    }
}

pub struct S3Backend {
    runtime: tokio::runtime::Runtime,
    client: S3Client,
    bucket: String,
    prefix: String,
}

impl S3Backend {
    pub fn from_remote(remote: Option<&str>) -> Result<Self, String> {
        let (bucket, prefix) = match remote {
            Some(remote) => parse_s3_remote(remote)?,
            None => {
                let bucket = env::var("BSJ_S3_BUCKET").map_err(|_| {
                    "missing BSJ_S3_BUCKET or --remote s3://bucket/prefix".to_string()
                })?;
                let prefix = env::var("BSJ_S3_PREFIX").unwrap_or_default();
                (bucket, prefix)
            }
        };

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("failed to create Tokio runtime: {error}"))?;
        let config = runtime
            .block_on(async { aws_config::defaults(BehaviorVersion::latest()).load().await });
        let client = S3Client::new(&config);

        Ok(Self {
            runtime,
            client,
            bucket,
            prefix: normalize_prefix(&prefix),
        })
    }

    fn remote_key(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}{}", self.prefix, key)
        }
    }

    fn relative_key(&self, key: &str) -> Option<String> {
        if self.prefix.is_empty() {
            Some(key.to_string())
        } else {
            key.strip_prefix(&self.prefix).map(ToString::to_string)
        }
    }
}

impl SyncBackend for S3Backend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
        let bucket = self.bucket.clone();
        let prefix = self.prefix.clone();
        self.runtime.block_on(async {
            let mut keys = BTreeSet::new();
            let mut continuation: Option<String> = None;
            loop {
                let response = self
                    .client
                    .list_objects_v2()
                    .bucket(&bucket)
                    .set_prefix((!prefix.is_empty()).then_some(prefix.clone()))
                    .set_continuation_token(continuation.clone())
                    .send()
                    .await
                    .map_err(|error| VaultError::Sync(format!("S3 list failed: {error}")))?;

                for object in response.contents() {
                    let Some(remote_key) = object.key() else {
                        continue;
                    };
                    let Some(relative_key) = self.relative_key(remote_key) else {
                        continue;
                    };
                    if classify_sync_key(&relative_key).is_some() {
                        keys.insert(relative_key);
                    }
                }

                if response.is_truncated().unwrap_or(false) {
                    continuation = response.next_continuation_token().map(ToString::to_string);
                } else {
                    break;
                }
            }
            Ok(keys)
        })
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        let bucket = self.bucket.clone();
        let remote_key = self.remote_key(key);
        self.runtime.block_on(async {
            let response = self
                .client
                .get_object()
                .bucket(&bucket)
                .key(&remote_key)
                .send()
                .await
                .map_err(|error| VaultError::Sync(format!("S3 get failed for {key}: {error}")))?;
            let body =
                response.body.collect().await.map_err(|error| {
                    VaultError::Sync(format!("S3 read failed for {key}: {error}"))
                })?;
            Ok(body.into_bytes().to_vec())
        })
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        let bucket = self.bucket.clone();
        let remote_key = self.remote_key(key);
        let body = ByteStream::from(bytes.to_vec());
        self.runtime.block_on(async {
            self.client
                .put_object()
                .bucket(&bucket)
                .key(&remote_key)
                .body(body)
                .send()
                .await
                .map_err(|error| VaultError::Sync(format!("S3 put failed for {key}: {error}")))?;
            Ok(())
        })
    }
}

pub struct WebDavBackend {
    base_url: Url,
    client: HttpClient,
    username: Option<String>,
    password: Option<String>,
    known_collections: HashSet<String>,
}

impl WebDavBackend {
    pub fn from_remote(remote: Option<&str>) -> Result<Self, String> {
        let raw_url = match remote {
            Some(remote) => remote.to_string(),
            None => env::var("BSJ_WEBDAV_URL").map_err(|_| {
                "missing BSJ_WEBDAV_URL or --remote https://server/path/".to_string()
            })?,
        };
        let mut base_url =
            Url::parse(&raw_url).map_err(|error| format!("invalid WebDAV URL: {error}"))?;
        match base_url.scheme() {
            "http" | "https" => {}
            _ => return Err("WebDAV URL must use http:// or https://".to_string()),
        }
        if !base_url.path().ends_with('/') {
            let normalized = format!("{}/", base_url.path());
            base_url.set_path(&normalized);
        }

        let username = env::var("BSJ_WEBDAV_USERNAME").ok();
        let password = env::var("BSJ_WEBDAV_PASSWORD").ok();
        if username.is_some() && password.is_none() {
            return Err("missing BSJ_WEBDAV_PASSWORD".to_string());
        }

        let client = HttpClient::builder()
            .build()
            .map_err(|error| format!("failed to create WebDAV client: {error}"))?;

        Ok(Self {
            base_url,
            client,
            username,
            password,
            known_collections: HashSet::new(),
        })
    }

    fn request(&self, method: Method, url: Url) -> RequestBuilder {
        let builder = self.client.request(method, url);
        match &self.username {
            Some(username) => builder.basic_auth(username, self.password.as_ref()),
            None => builder,
        }
    }

    fn file_url(&self, key: &str) -> VaultResult<Url> {
        self.base_url
            .join(key)
            .map_err(|error| VaultError::Sync(format!("invalid WebDAV file URL: {error}")))
    }

    fn collection_url(&self, collection: &str) -> VaultResult<Url> {
        if collection.is_empty() {
            return Ok(self.base_url.clone());
        }
        let normalized = format!("{}/", collection.trim_matches('/'));
        self.base_url
            .join(&normalized)
            .map_err(|error| VaultError::Sync(format!("invalid WebDAV collection URL: {error}")))
    }

    fn ensure_parent_collections(&mut self, key: &str) -> VaultResult<()> {
        let Some(parent) = Path::new(key).parent() else {
            return Ok(());
        };
        if parent.as_os_str().is_empty() {
            return Ok(());
        }

        let mut current = String::new();
        for component in parent.components() {
            let Component::Normal(segment) = component else {
                return Err(VaultError::Sync(
                    "invalid WebDAV collection path".to_string(),
                ));
            };
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(&segment.to_string_lossy());
            if !self.known_collections.insert(current.clone()) {
                continue;
            }

            let response = self
                .request(
                    Method::from_bytes(b"MKCOL").expect("MKCOL"),
                    self.collection_url(&current)?,
                )
                .send()
                .map_err(|error| {
                    VaultError::Sync(format!("WebDAV MKCOL failed for {current}: {error}"))
                })?;

            let status = response.status();
            if !matches!(
                status,
                StatusCode::CREATED
                    | StatusCode::OK
                    | StatusCode::NO_CONTENT
                    | StatusCode::METHOD_NOT_ALLOWED
            ) {
                return Err(VaultError::Sync(format!(
                    "WebDAV MKCOL failed for {current}: {status}"
                )));
            }
        }

        Ok(())
    }

    fn list_recursive(
        &self,
        prefix: &str,
        visited: &mut HashSet<String>,
        keys: &mut BTreeSet<String>,
    ) -> VaultResult<()> {
        let normalized_prefix = prefix.trim_matches('/').to_string();
        if !visited.insert(normalized_prefix.clone()) {
            return Ok(());
        }

        let response = self
            .request(
                Method::from_bytes(b"PROPFIND").expect("PROPFIND"),
                self.collection_url(&normalized_prefix)?,
            )
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype /></d:prop></d:propfind>"#,
            )
            .send()
            .map_err(|error| {
                VaultError::Sync(format!(
                    "WebDAV PROPFIND failed for {}: {error}",
                    if normalized_prefix.is_empty() {
                        "/"
                    } else {
                        normalized_prefix.as_str()
                    }
                ))
            })?;

        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        if response.status().as_u16() != 207 && !response.status().is_success() {
            return Err(VaultError::Sync(format!(
                "WebDAV PROPFIND failed for {}: {}",
                if normalized_prefix.is_empty() {
                    "/"
                } else {
                    normalized_prefix.as_str()
                },
                response.status()
            )));
        }

        let xml = response
            .text()
            .map_err(|error| VaultError::Sync(format!("WebDAV response read failed: {error}")))?;
        for resource in self.parse_propfind(&xml)? {
            if resource.relative_key.is_empty() || resource.relative_key == normalized_prefix {
                continue;
            }
            if resource.is_collection {
                if should_descend_collection(&resource.relative_key) {
                    self.list_recursive(&resource.relative_key, visited, keys)?;
                }
                continue;
            }
            if classify_sync_key(&resource.relative_key).is_some() {
                keys.insert(resource.relative_key);
            }
        }

        Ok(())
    }

    fn parse_propfind(&self, xml: &str) -> VaultResult<Vec<DavResource>> {
        let document = Document::parse(xml)
            .map_err(|error| VaultError::Sync(format!("WebDAV XML parse failed: {error}")))?;
        let mut resources = Vec::new();

        for response in document.descendants().filter(|node| {
            node.is_element() && node.tag_name().name().eq_ignore_ascii_case("response")
        }) {
            let Some(href) = response.descendants().find_map(|node| {
                (node.is_element() && node.tag_name().name().eq_ignore_ascii_case("href"))
                    .then(|| node.text())
                    .flatten()
            }) else {
                continue;
            };

            let relative_key = self.href_to_relative_key(href)?;
            let is_collection = response.descendants().any(|node| {
                node.is_element() && node.tag_name().name().eq_ignore_ascii_case("collection")
            });

            resources.push(DavResource {
                relative_key,
                is_collection,
            });
        }

        Ok(resources)
    }

    fn href_to_relative_key(&self, href: &str) -> VaultResult<String> {
        let joined = self
            .base_url
            .join(href)
            .map_err(|error| VaultError::Sync(format!("invalid WebDAV href {href}: {error}")))?;
        let base_path = self.base_url.path();
        let path = joined.path();

        let stripped = if let Some(stripped) = path.strip_prefix(base_path) {
            stripped
        } else {
            let trimmed_base = base_path.trim_end_matches('/');
            path.strip_prefix(trimmed_base)
                .ok_or_else(|| VaultError::Sync(format!("unexpected WebDAV href {href}")))?
        };

        Ok(stripped
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string())
    }
}

impl SyncBackend for WebDavBackend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
        let mut visited = HashSet::new();
        let mut keys = BTreeSet::new();
        self.list_recursive("", &mut visited, &mut keys)?;
        Ok(keys)
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        let response = self
            .request(Method::GET, self.file_url(key)?)
            .send()
            .map_err(|error| VaultError::Sync(format!("WebDAV GET failed for {key}: {error}")))?;
        if !response.status().is_success() {
            return Err(VaultError::Sync(format!(
                "WebDAV GET failed for {key}: {}",
                response.status()
            )));
        }
        response
            .bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(|error| VaultError::Sync(format!("WebDAV read failed for {key}: {error}")))
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        self.ensure_parent_collections(key)?;
        let response = self
            .request(Method::PUT, self.file_url(key)?)
            .body(bytes.to_vec())
            .send()
            .map_err(|error| VaultError::Sync(format!("WebDAV PUT failed for {key}: {error}")))?;
        if !response.status().is_success() {
            return Err(VaultError::Sync(format!(
                "WebDAV PUT failed for {key}: {}",
                response.status()
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct DavResource {
    relative_key: String,
    is_collection: bool,
}

pub fn sync_root<B: SyncBackend>(
    local_root: &Path,
    backend: &mut B,
) -> VaultResult<BackendSyncReport> {
    let mut remote_keys = backend.list_keys()?;
    ensure_remote_compatibility(local_root, backend, &remote_keys)?;
    remote_keys = backend.list_keys()?;

    let mut local_inventory = list_local_inventory(local_root)?;
    let mut pulled = 0usize;
    for key in &remote_keys {
        if local_inventory.keys.contains(key) {
            continue;
        }
        let bytes = backend.read(key)?;
        write_local_key(local_root, key, &bytes)?;
        if matches!(classify_sync_key(key), Some(SyncObjectKind::Revision)) {
            pulled += 1;
        }
    }

    local_inventory = list_local_inventory(local_root)?;
    remote_keys = backend.list_keys()?;

    let mut pushed = 0usize;
    for key in &local_inventory.keys {
        if remote_keys.contains(key) {
            continue;
        }
        let bytes = fs::read(sync_key_to_path(local_root, key)?)?;
        backend.write(key, &bytes)?;
        if local_inventory.revision_keys.contains(key) {
            pushed += 1;
        }
    }

    local_inventory = list_local_inventory(local_root)?;
    remote_keys = backend.list_keys()?;
    ensure_shared_revision_bytes_match(local_root, backend, &local_inventory, &remote_keys)?;

    Ok(BackendSyncReport { pulled, pushed })
}

pub fn looks_like_s3_remote(remote: &str) -> bool {
    remote.starts_with("s3://")
}

pub fn looks_like_webdav_remote(remote: &str) -> bool {
    remote.starts_with("http://") || remote.starts_with("https://")
}

fn should_descend_collection(relative_key: &str) -> bool {
    matches!(
        relative_key.split('/').next(),
        Some("devices") | Some("entries")
    )
}

fn parse_s3_remote(remote: &str) -> Result<(String, String), String> {
    let Some(without_scheme) = remote.strip_prefix("s3://") else {
        return Err("S3 remote must use s3://bucket/prefix".to_string());
    };
    let (bucket, prefix) = match without_scheme.split_once('/') {
        Some((bucket, prefix)) => (bucket, prefix),
        None => (without_scheme, ""),
    };
    if bucket.is_empty() {
        return Err("S3 remote must include a bucket name".to_string());
    }
    Ok((bucket.to_string(), prefix.trim_matches('/').to_string()))
}

fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}/")
    }
}

fn ensure_remote_compatibility<B: SyncBackend>(
    local_root: &Path,
    backend: &mut B,
    remote_keys: &BTreeSet<String>,
) -> VaultResult<()> {
    let local_vault_json = fs::read(local_root.join("vault.json"))?;
    if !remote_keys.contains("vault.json") {
        backend.write("vault.json", &local_vault_json)?;
        return Ok(());
    }

    let local_metadata: VaultMetadata = serde_json::from_slice(&local_vault_json)?;
    let remote_metadata: VaultMetadata = serde_json::from_slice(&backend.read("vault.json")?)?;
    if !vault_metadata_compatible(&local_metadata, &remote_metadata) {
        return Err(VaultError::InvalidFormat(
            "remote vault metadata is incompatible".to_string(),
        ));
    }
    Ok(())
}

fn vault_metadata_compatible(local: &VaultMetadata, remote: &VaultMetadata) -> bool {
    local.version == remote.version
        && local.created_at == remote.created_at
        && local.kdf.salt_hex == remote.kdf.salt_hex
        && local.kdf.memory_kib == remote.kdf.memory_kib
        && local.kdf.iterations == remote.kdf.iterations
        && local.kdf.parallelism == remote.kdf.parallelism
        && local.options.epoch_date == remote.options.epoch_date
}

fn ensure_shared_revision_bytes_match<B: SyncBackend>(
    local_root: &Path,
    backend: &mut B,
    local_inventory: &SyncInventory,
    remote_keys: &BTreeSet<String>,
) -> VaultResult<()> {
    for key in local_inventory.revision_keys.intersection(remote_keys) {
        let local_bytes = fs::read(sync_key_to_path(local_root, key)?)?;
        let remote_bytes = backend.read(key)?;
        if local_bytes != remote_bytes {
            return Err(VaultError::InvalidFormat(format!(
                "sync collision for {key}"
            )));
        }
    }
    Ok(())
}

fn list_local_inventory(root: &Path) -> VaultResult<SyncInventory> {
    let mut inventory = SyncInventory::default();
    collect_local_inventory(root, root, &mut inventory)?;
    Ok(inventory)
}

fn collect_local_inventory(
    root: &Path,
    current: &Path,
    inventory: &mut SyncInventory,
) -> VaultResult<()> {
    if !current.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_local_inventory(root, &path, inventory)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| VaultError::Sync("invalid sync path".to_string()))?;
        let key = relative_path_to_key(relative)?;
        let Some(kind) = classify_sync_key(&key) else {
            continue;
        };
        inventory.keys.insert(key.clone());
        if kind == SyncObjectKind::Revision {
            inventory.revision_keys.insert(key);
        }
    }

    Ok(())
}

fn classify_sync_key(key: &str) -> Option<SyncObjectKind> {
    if key == "vault.json" {
        return Some(SyncObjectKind::Metadata);
    }

    if let Some(device_file) = key.strip_prefix("devices/") {
        if !device_file.is_empty() && !device_file.contains('/') && device_file.ends_with(".json") {
            return Some(SyncObjectKind::Metadata);
        }
        return None;
    }

    let parts = key.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != "entries" {
        return None;
    }
    if parts[1].len() != 4 || parts[1].parse::<u32>().is_err() {
        return None;
    }
    if NaiveDate::parse_from_str(parts[2], "%Y-%m-%d").is_err() {
        return None;
    }
    if !parts[3].starts_with("rev-") || !parts[3].ends_with(".bsj.enc") {
        return None;
    }

    Some(SyncObjectKind::Revision)
}

fn relative_path_to_key(relative: &Path) -> VaultResult<String> {
    let mut segments = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(segment) => segments.push(segment.to_string_lossy().into_owned()),
            _ => {
                return Err(VaultError::Sync(
                    "sync file path contains invalid components".to_string(),
                ));
            }
        }
    }

    if segments.is_empty() {
        return Err(VaultError::Sync("empty sync file path".to_string()));
    }

    Ok(segments.join("/"))
}

fn sync_key_to_path(root: &Path, key: &str) -> VaultResult<PathBuf> {
    let Some(_) = classify_sync_key(key) else {
        return Err(VaultError::Sync(format!("invalid sync key {key}")));
    };
    if key.contains('\\') {
        return Err(VaultError::Sync(format!("invalid sync key {key}")));
    }

    let mut path = PathBuf::from(root);
    for segment in key.split('/') {
        if segment.is_empty() || matches!(segment, "." | "..") {
            return Err(VaultError::Sync(format!("invalid sync key {key}")));
        }
        path.push(segment);
    }
    Ok(path)
}

fn write_local_key(root: &Path, key: &str, bytes: &[u8]) -> VaultResult<()> {
    atomic_write(&sync_key_to_path(root, key)?, bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> VaultResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| VaultError::Sync("missing sync file parent directory".to_string()))?;
    fs::create_dir_all(parent)?;

    let mut suffix = [0u8; 4];
    OsRng.fill_bytes(&mut suffix);
    let tmp_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sync"),
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
    use crate::vault::{create_vault, register_device, unlock_vault_with_device};
    use secrecy::SecretString;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    #[derive(Default)]
    struct MemoryBackend {
        objects: BTreeMap<String, Vec<u8>>,
    }

    impl MemoryBackend {
        fn insert(&mut self, key: &str, bytes: Vec<u8>) {
            self.objects.insert(key.to_string(), bytes);
        }
    }

    impl SyncBackend for MemoryBackend {
        fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
            Ok(self
                .objects
                .keys()
                .filter(|key| classify_sync_key(key).is_some())
                .cloned()
                .collect())
        }

        fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
            self.objects
                .get(key)
                .cloned()
                .ok_or_else(|| VaultError::Sync(format!("missing test object {key}")))
        }

        fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
            self.objects.insert(key.to_string(), bytes.to_vec());
            Ok(())
        }
    }

    #[test]
    fn parse_and_normalize_s3_remote() {
        let (bucket, prefix) = parse_s3_remote("s3://journal-sync/archive/2026").expect("parse");
        assert_eq!(bucket, "journal-sync");
        assert_eq!(prefix, "archive/2026");
        assert_eq!(normalize_prefix(&prefix), "archive/2026/");
    }

    #[test]
    fn sync_key_path_mapping_round_trips() {
        let root = Path::new("/tmp/vault");
        let key = "entries/2026/2026-03-16/rev-device-a-000001.bsj.enc";
        let path = sync_key_to_path(root, key).expect("path");
        assert_eq!(
            path,
            PathBuf::from("/tmp/vault/entries/2026/2026-03-16/rev-device-a-000001.bsj.enc")
        );
        let relative = path.strip_prefix(root).expect("relative");
        assert_eq!(relative_path_to_key(relative).expect("key"), key);
    }

    #[test]
    fn invalid_sync_key_is_rejected() {
        let root = Path::new("/tmp/vault");
        let error = sync_key_to_path(root, "../vault.json").expect_err("reject");
        assert!(error.to_string().contains("invalid sync key"));
    }

    #[test]
    fn sync_root_reconciles_pull_and_push_against_backend() {
        let dir = tempdir().expect("tempdir");
        let local_root = dir.path().join("local");
        let remote_root = dir.path().join("remote-source");
        let passphrase = SecretString::new("correct horse battery staple".into());

        create_vault(&local_root, &passphrase, None, "Local").expect("create local");
        create_vault(&remote_root, &passphrase, None, "Remote").expect("create remote");
        let local_metadata = fs::read(local_root.join("vault.json")).expect("local metadata");
        fs::write(remote_root.join("vault.json"), local_metadata).expect("align metadata");

        let local =
            unlock_vault_with_device(&local_root, &passphrase, "device-a").expect("unlock local");
        let remote =
            unlock_vault_with_device(&remote_root, &passphrase, "device-b").expect("unlock remote");
        register_device(&local_root, "device-a", "Local").expect("register local");
        register_device(&remote_root, "device-b", "Remote").expect("register remote");

        local
            .save_revision(
                NaiveDate::from_ymd_opt(2026, 3, 16).expect("date"),
                "local entry",
            )
            .expect("save local");
        remote
            .save_revision(
                NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"),
                "remote entry",
            )
            .expect("save remote");

        let mut backend = MemoryBackend::default();
        for key in list_local_inventory(&remote_root)
            .expect("remote inventory")
            .keys
        {
            backend.insert(
                &key,
                fs::read(sync_key_to_path(&remote_root, &key).expect("remote path"))
                    .expect("remote bytes"),
            );
        }

        let report = sync_root(&local_root, &mut backend).expect("sync");
        assert_eq!(report.pulled, 1);
        assert_eq!(report.pushed, 1);

        assert!(
            backend
                .objects
                .contains_key("entries/2026/2026-03-16/rev-device-a-000001.bsj.enc")
        );
        assert!(
            local_root
                .join("entries/2026/2026-03-17/rev-device-b-000001.bsj.enc")
                .exists()
        );
    }

    #[test]
    fn s3_backend_smoke_lists_when_env_present() {
        if env::var("BSJ_S3_BUCKET").is_err() {
            eprintln!("skipping S3 integration smoke test: BSJ_S3_BUCKET not set");
            return;
        }
        let mut backend = S3Backend::from_remote(None).expect("backend");
        backend.list_keys().expect("list");
    }

    #[test]
    fn webdav_backend_smoke_lists_when_env_present() {
        if env::var("BSJ_WEBDAV_URL").is_err() {
            eprintln!("skipping WebDAV integration smoke test: BSJ_WEBDAV_URL not set");
            return;
        }
        let mut backend = WebDavBackend::from_remote(None).expect("backend");
        backend.list_keys().expect("list");
    }

    #[test]
    fn webdav_url_detection_matches_http_schemes() {
        assert!(looks_like_webdav_remote("https://dav.example.com/bsj/"));
        assert!(looks_like_webdav_remote("http://dav.example.com/bsj/"));
        assert!(!looks_like_webdav_remote("/Users/sean/Documents/bsj-sync"));
    }

    #[test]
    fn s3_url_detection_matches_s3_scheme() {
        assert!(looks_like_s3_remote("s3://bucket/prefix"));
        assert!(!looks_like_s3_remote("https://example.com/bucket"));
    }

    #[test]
    fn webdav_constructor_normalizes_base_url() {
        let backend =
            WebDavBackend::from_remote(Some("https://example.com/bsj-test")).expect("backend");
        assert_eq!(backend.base_url.as_str(), "https://example.com/bsj-test/");
    }
}
