use crate::{
    secure_fs,
    vault::{VaultError, VaultMetadata, VaultResult},
};
use aws_config::BehaviorVersion;
use aws_sdk_s3::{Client as S3Client, primitives::ByteStream};
use chrono::NaiveDate;
use reqwest::{
    Method, StatusCode, Url,
    blocking::{Client as HttpClient, RequestBuilder},
};
use roxmltree::Document;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    env, fs,
    path::{Component, Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendSyncReport {
    pub pulled: usize,
    pub pushed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncPreviewReport {
    pub local_revisions: usize,
    pub remote_revisions: usize,
    pub local_only_revisions: usize,
    pub remote_only_revisions: usize,
    pub shared_revisions: usize,
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

const REQUEST_MIN_INTERVAL: Duration = Duration::from_millis(150);
const REQUEST_MAX_ATTEMPTS: usize = 4;
const REQUEST_INITIAL_BACKOFF: Duration = Duration::from_millis(200);
const GOOGLE_DRIVE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";
const GOOGLE_DRIVE_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3";
const GOOGLE_DRIVE_DEFAULT_PARENT: &str = "appDataFolder";
const DROPBOX_API_BASE: &str = "https://api.dropboxapi.com/2";
const DROPBOX_CONTENT_BASE: &str = "https://content.dropboxapi.com/2";
const DROPBOX_TOKEN_URL: &str = "https://api.dropboxapi.com/oauth2/token";
const DROPBOX_DEFAULT_ROOT: &str = "/BlueScreenJournal-Sync";

#[derive(Default)]
struct RequestThrottle {
    last_request_at: Option<Instant>,
}

impl RequestThrottle {
    fn wait_for_turn(&mut self) {
        if let Some(last_request_at) = self.last_request_at {
            let elapsed = last_request_at.elapsed();
            if elapsed < REQUEST_MIN_INTERVAL {
                thread::sleep(REQUEST_MIN_INTERVAL - elapsed);
            }
        }
        self.last_request_at = Some(Instant::now());
    }
}

fn retry_sync_operation<T, F>(label: &str, mut operation: F) -> VaultResult<T>
where
    F: FnMut() -> VaultResult<T>,
{
    let mut delay = REQUEST_INITIAL_BACKOFF;
    for attempt in 1..=REQUEST_MAX_ATTEMPTS {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if attempt < REQUEST_MAX_ATTEMPTS && is_retryable_sync_error(&error) => {
                log::warn!(
                    "{label} failed on attempt {attempt}/{REQUEST_MAX_ATTEMPTS}: {error}; retrying"
                );
                thread::sleep(delay);
                delay = delay.saturating_mul(2);
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("retry loop returns on success or final error")
}

fn is_retryable_sync_error(error: &VaultError) -> bool {
    matches!(error, VaultError::Sync(_) | VaultError::Io(_))
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
    throttle: RequestThrottle,
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
            throttle: RequestThrottle::default(),
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

    fn list_keys_once(&mut self) -> VaultResult<BTreeSet<String>> {
        self.throttle.wait_for_turn();
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

    fn read_once(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        self.throttle.wait_for_turn();
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

    fn write_once(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        self.throttle.wait_for_turn();
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

impl SyncBackend for S3Backend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
        retry_sync_operation("S3 list", || self.list_keys_once())
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        retry_sync_operation(&format!("S3 read {key}"), || self.read_once(key))
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        retry_sync_operation(&format!("S3 write {key}"), || self.write_once(key, bytes))
    }
}

pub struct WebDavBackend {
    base_url: Url,
    client: HttpClient,
    username: Option<String>,
    password: Option<String>,
    known_collections: HashSet<String>,
    throttle: RequestThrottle,
}

impl WebDavBackend {
    pub fn from_remote(remote: Option<&str>) -> Result<Self, String> {
        let raw_url = match remote {
            Some(remote) => remote.to_string(),
            None => env::var("BSJ_WEBDAV_URL").map_err(|_| {
                "missing BSJ_WEBDAV_URL or --remote https://server/path/".to_string()
            })?,
        };
        let mut base_url = validate_webdav_base_url(&raw_url, insecure_webdav_http_allowed())?;
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
            throttle: RequestThrottle::default(),
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
                .throttled_request(
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
        &mut self,
        prefix: &str,
        visited: &mut HashSet<String>,
        keys: &mut BTreeSet<String>,
    ) -> VaultResult<()> {
        let normalized_prefix = prefix.trim_matches('/').to_string();
        if !visited.insert(normalized_prefix.clone()) {
            return Ok(());
        }

        let response = self
            .throttled_request(
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

    fn throttled_request(&mut self, method: Method, url: Url) -> RequestBuilder {
        self.throttle.wait_for_turn();
        self.request(method, url)
    }

    fn list_keys_once(&mut self) -> VaultResult<BTreeSet<String>> {
        let mut visited = HashSet::new();
        let mut keys = BTreeSet::new();
        self.list_recursive("", &mut visited, &mut keys)?;
        Ok(keys)
    }

    fn read_once(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        let response = self
            .throttled_request(Method::GET, self.file_url(key)?)
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

    fn write_once(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        self.ensure_parent_collections(key)?;
        let response = self
            .throttled_request(Method::PUT, self.file_url(key)?)
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
        retry_sync_operation("WebDAV list", || self.list_keys_once())
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        retry_sync_operation(&format!("WebDAV read {key}"), || self.read_once(key))
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        retry_sync_operation(&format!("WebDAV write {key}"), || {
            self.write_once(key, bytes)
        })
    }
}

#[derive(Clone, Debug)]
struct DavResource {
    relative_key: String,
    is_collection: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OAuthCredentialBundle {
    pub access_token: Option<SecretString>,
    pub refresh_token: Option<SecretString>,
    pub client_id: Option<SecretString>,
    pub client_secret: Option<SecretString>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OAuthCredentialPresence {
    pub access_token: bool,
    pub refresh_token: bool,
    pub client_id: bool,
    pub client_secret: bool,
}

impl OAuthCredentialPresence {
    pub fn ready(self) -> bool {
        self.access_token || (self.refresh_token && self.client_id && self.client_secret)
    }

    pub fn source_label(self, keychain: Self) -> &'static str {
        let env_ready = self.ready();
        let keychain_ready = keychain.ready();
        match (env_ready, keychain_ready) {
            (true, true) => "ENV+KEYCHAIN",
            (true, false) => "ENV",
            (false, true) => "KEYCHAIN",
            (false, false) => "MISSING",
        }
    }
}

impl OAuthCredentialBundle {
    pub fn from_env(
        access_env: &'static str,
        refresh_env: &'static str,
        client_id_env: &'static str,
        client_secret_env: &'static str,
    ) -> Self {
        Self {
            access_token: env::var(access_env)
                .ok()
                .map(|value| SecretString::new(value.into_boxed_str())),
            refresh_token: env::var(refresh_env)
                .ok()
                .map(|value| SecretString::new(value.into_boxed_str())),
            client_id: env::var(client_id_env)
                .ok()
                .map(|value| SecretString::new(value.into_boxed_str())),
            client_secret: env::var(client_secret_env)
                .ok()
                .map(|value| SecretString::new(value.into_boxed_str())),
        }
        .normalized()
    }

    pub fn normalized(mut self) -> Self {
        self.access_token = normalize_secret_option(self.access_token);
        self.refresh_token = normalize_secret_option(self.refresh_token);
        self.client_id = normalize_secret_option(self.client_id);
        self.client_secret = normalize_secret_option(self.client_secret);
        self
    }

    pub fn fill_missing_from(mut self, fallback: Self) -> Self {
        let fallback = fallback.normalized();
        self = self.normalized();
        if self.access_token.is_none() {
            self.access_token = fallback.access_token;
        }
        if self.refresh_token.is_none() {
            self.refresh_token = fallback.refresh_token;
        }
        if self.client_id.is_none() {
            self.client_id = fallback.client_id;
        }
        if self.client_secret.is_none() {
            self.client_secret = fallback.client_secret;
        }
        self
    }

    pub fn presence(&self) -> OAuthCredentialPresence {
        OAuthCredentialPresence {
            access_token: secret_option_is_present(self.access_token.as_ref()),
            refresh_token: secret_option_is_present(self.refresh_token.as_ref()),
            client_id: secret_option_is_present(self.client_id.as_ref()),
            client_secret: secret_option_is_present(self.client_secret.as_ref()),
        }
    }

    pub fn ready(&self) -> bool {
        self.presence().ready()
    }
}

#[derive(Clone, Debug)]
struct OAuthAccessState {
    access_token: SecretString,
    refresh_token: Option<SecretString>,
    client_id: Option<SecretString>,
    client_secret: Option<SecretString>,
    provider_label: &'static str,
}

impl OAuthAccessState {
    fn from_credentials(
        provider_label: &'static str,
        credentials: OAuthCredentialBundle,
        access_env: &'static str,
        refresh_env: &'static str,
        client_id_env: &'static str,
        client_secret_env: &'static str,
    ) -> Result<Self, String> {
        let credentials = credentials.normalized();
        if !credentials.ready() {
            return Err(format!(
                "missing {access_env}; set {access_env} (or configure {refresh_env}, {client_id_env}, and {client_secret_env})"
            ));
        }

        Ok(Self {
            access_token: credentials
                .access_token
                .unwrap_or_else(|| SecretString::new(String::new().into_boxed_str())),
            refresh_token: credentials.refresh_token,
            client_id: credentials.client_id,
            client_secret: credentials.client_secret,
            provider_label,
        })
    }

    fn token(&self) -> &str {
        self.access_token.expose_secret()
    }

    fn can_refresh(&self) -> bool {
        self.refresh_token.is_some() && self.client_id.is_some() && self.client_secret.is_some()
    }

    fn refresh(&mut self, client: &HttpClient, token_url: &str) -> VaultResult<()> {
        let refresh_token = self.refresh_token.as_ref().ok_or_else(|| {
            VaultError::Sync(format!(
                "{} access token expired and refresh token was not configured",
                self.provider_label
            ))
        })?;
        let client_id = self.client_id.as_ref().ok_or_else(|| {
            VaultError::Sync(format!(
                "{} access token expired and OAuth client id was not configured",
                self.provider_label
            ))
        })?;
        let client_secret = self.client_secret.as_ref().ok_or_else(|| {
            VaultError::Sync(format!(
                "{} access token expired and OAuth client secret was not configured",
                self.provider_label
            ))
        })?;

        let response = client
            .post(token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.expose_secret()),
                ("client_id", client_id.expose_secret()),
                ("client_secret", client_secret.expose_secret()),
            ])
            .send()
            .map_err(|error| {
                VaultError::Sync(format!(
                    "{} token refresh request failed: {error}",
                    self.provider_label
                ))
            })?;

        if !response.status().is_success() {
            return Err(response_to_sync_error(
                self.provider_label,
                "token refresh",
                response,
            ));
        }

        let payload: OAuthRefreshResponse = response.json().map_err(|error| {
            VaultError::Sync(format!(
                "{} token refresh response parse failed: {error}",
                self.provider_label
            ))
        })?;
        self.access_token = SecretString::new(payload.access_token.into_boxed_str());
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct OAuthRefreshResponse {
    access_token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum GoogleDriveRoot {
    AppDataFolder,
    FolderId(String),
}

impl GoogleDriveRoot {
    fn parent_for_create(&self) -> String {
        match self {
            Self::AppDataFolder => GOOGLE_DRIVE_DEFAULT_PARENT.to_string(),
            Self::FolderId(folder_id) => folder_id.clone(),
        }
    }

    fn parent_clause(&self) -> String {
        match self {
            Self::AppDataFolder => {
                format!("'{GOOGLE_DRIVE_DEFAULT_PARENT}' in parents")
            }
            Self::FolderId(folder_id) => format!("'{}' in parents", folder_id),
        }
    }

    fn spaces_param(&self) -> Option<&'static str> {
        match self {
            Self::AppDataFolder => Some("appDataFolder"),
            Self::FolderId(_) => None,
        }
    }
}

pub struct GoogleDriveBackend {
    client: HttpClient,
    auth: OAuthAccessState,
    root: GoogleDriveRoot,
    throttle: RequestThrottle,
    file_ids: HashMap<String, String>,
}

impl GoogleDriveBackend {
    #[allow(dead_code)]
    pub fn from_remote(remote: Option<&str>) -> Result<Self, String> {
        let credentials = OAuthCredentialBundle::from_env(
            "BSJ_GDRIVE_ACCESS_TOKEN",
            "BSJ_GDRIVE_REFRESH_TOKEN",
            "BSJ_GDRIVE_CLIENT_ID",
            "BSJ_GDRIVE_CLIENT_SECRET",
        );
        Self::from_remote_with_credentials(remote, credentials)
    }

    pub fn from_remote_with_credentials(
        remote: Option<&str>,
        credentials: OAuthCredentialBundle,
    ) -> Result<Self, String> {
        let client = HttpClient::builder()
            .build()
            .map_err(|error| format!("failed to create Google Drive client: {error}"))?;
        let mut auth = OAuthAccessState::from_credentials(
            "Google Drive",
            credentials,
            "BSJ_GDRIVE_ACCESS_TOKEN",
            "BSJ_GDRIVE_REFRESH_TOKEN",
            "BSJ_GDRIVE_CLIENT_ID",
            "BSJ_GDRIVE_CLIENT_SECRET",
        )?;
        let root = parse_google_drive_remote(remote)?;
        if auth.token().trim().is_empty() {
            auth.refresh(&client, GOOGLE_DRIVE_TOKEN_URL)
                .map_err(|error| format!("Google Drive token refresh failed: {error}"))?;
        }

        Ok(Self {
            client,
            auth,
            root,
            throttle: RequestThrottle::default(),
            file_ids: HashMap::new(),
        })
    }

    fn with_auth<F>(
        &mut self,
        label: &str,
        mut build: F,
    ) -> VaultResult<reqwest::blocking::Response>
    where
        F: FnMut(&HttpClient, &str) -> RequestBuilder,
    {
        let mut refreshed = false;
        loop {
            self.throttle.wait_for_turn();
            let response = build(&self.client, self.auth.token())
                .send()
                .map_err(|error| {
                    VaultError::Sync(format!("Google Drive {label} request failed: {error}"))
                })?;

            if response.status() == StatusCode::UNAUTHORIZED
                && !refreshed
                && self.auth.can_refresh()
            {
                self.auth.refresh(&self.client, GOOGLE_DRIVE_TOKEN_URL)?;
                refreshed = true;
                continue;
            }

            return Ok(response);
        }
    }

    fn list_query(&self) -> String {
        format!(
            "trashed = false and {} and appProperties has {{ key='bsj_key' and value != '' }}",
            self.root.parent_clause()
        )
    }

    fn key_query(&self, key: &str) -> String {
        let escaped = key.replace('\'', "\\'");
        format!(
            "trashed = false and {} and appProperties has {{ key='bsj_key' and value='{}' }}",
            self.root.parent_clause(),
            escaped
        )
    }

    fn list_files(
        &mut self,
        query: &str,
        page_token: Option<&str>,
        page_size: usize,
    ) -> VaultResult<GoogleDriveListResponse> {
        let mut params = vec![
            ("q", query.to_string()),
            (
                "fields",
                "nextPageToken,files(id,appProperties)".to_string(),
            ),
            ("pageSize", page_size.to_string()),
        ];
        if let Some(page_token) = page_token {
            params.push(("pageToken", page_token.to_string()));
        }
        if let Some(spaces) = self.root.spaces_param() {
            params.push(("spaces", spaces.to_string()));
        }

        let response = self.with_auth("list", |client, token| {
            client
                .get(format!("{GOOGLE_DRIVE_API_BASE}/files"))
                .bearer_auth(token)
                .query(&params)
        })?;
        if !response.status().is_success() {
            return Err(response_to_sync_error("Google Drive", "list", response));
        }
        response
            .json::<GoogleDriveListResponse>()
            .map_err(|error| VaultError::Sync(format!("Google Drive list parse failed: {error}")))
    }

    fn find_file_id(&mut self, key: &str) -> VaultResult<Option<String>> {
        if let Some(id) = self.file_ids.get(key) {
            return Ok(Some(id.clone()));
        }

        let response = self.list_files(&self.key_query(key), None, 1)?;
        let id = response.files.into_iter().next().map(|item| item.id);
        if let Some(id) = &id {
            self.file_ids.insert(key.to_string(), id.clone());
        }
        Ok(id)
    }

    fn list_keys_once(&mut self) -> VaultResult<BTreeSet<String>> {
        let mut keys = BTreeSet::new();
        let mut page_token = None::<String>;
        loop {
            let response = self.list_files(&self.list_query(), page_token.as_deref(), 1000)?;
            for file in response.files {
                let Some(key) = file
                    .app_properties
                    .as_ref()
                    .and_then(|map| map.get("bsj_key"))
                    .cloned()
                else {
                    continue;
                };
                if classify_sync_key(&key).is_some() {
                    self.file_ids.insert(key.clone(), file.id);
                    keys.insert(key);
                }
            }
            page_token = response.next_page_token;
            if page_token.is_none() {
                break;
            }
        }
        Ok(keys)
    }

    fn read_once(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        let id = self
            .find_file_id(key)?
            .ok_or_else(|| VaultError::Sync(format!("Google Drive object not found for {key}")))?;
        let response = self.with_auth("read", |client, token| {
            client
                .get(format!("{GOOGLE_DRIVE_API_BASE}/files/{id}"))
                .bearer_auth(token)
                .query(&[("alt", "media")])
        })?;
        if !response.status().is_success() {
            return Err(response_to_sync_error("Google Drive", "read", response));
        }
        response
            .bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(|error| VaultError::Sync(format!("Google Drive read failed: {error}")))
    }

    fn write_once(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        if let Some(id) = self.find_file_id(key)? {
            let response = self.with_auth("update", |client, token| {
                client
                    .patch(format!("{GOOGLE_DRIVE_UPLOAD_BASE}/files/{id}"))
                    .bearer_auth(token)
                    .query(&[("uploadType", "media")])
                    .header("Content-Type", "application/octet-stream")
                    .body(bytes.to_vec())
            })?;
            if !response.status().is_success() {
                return Err(response_to_sync_error("Google Drive", "update", response));
            }
            return Ok(());
        }

        let metadata = serde_json::json!({
            "name": google_drive_object_name(key),
            "parents": [self.root.parent_for_create()],
            "appProperties": { "bsj_key": key },
        });
        let metadata_json = serde_json::to_string(&metadata)
            .map_err(|error| VaultError::Sync(format!("Google Drive metadata failed: {error}")))?;
        let boundary = format!("bsj-boundary-{}", hex::encode(rand::random::<[u8; 6]>()));
        let payload = google_drive_multipart_payload(&boundary, &metadata_json, bytes);
        let response = self.with_auth("create", |client, token| {
            client
                .post(format!("{GOOGLE_DRIVE_UPLOAD_BASE}/files"))
                .bearer_auth(token)
                .query(&[("uploadType", "multipart")])
                .header(
                    "Content-Type",
                    format!("multipart/related; boundary={boundary}"),
                )
                .body(payload.clone())
        })?;
        if !response.status().is_success() {
            return Err(response_to_sync_error("Google Drive", "create", response));
        }
        let created: GoogleDriveCreateResponse = response.json().map_err(|error| {
            VaultError::Sync(format!("Google Drive create parse failed: {error}"))
        })?;
        self.file_ids.insert(key.to_string(), created.id);
        Ok(())
    }
}

impl SyncBackend for GoogleDriveBackend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
        retry_sync_operation("Google Drive list", || self.list_keys_once())
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        retry_sync_operation(&format!("Google Drive read {key}"), || self.read_once(key))
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        retry_sync_operation(&format!("Google Drive write {key}"), || {
            self.write_once(key, bytes)
        })
    }
}

#[derive(Debug, Deserialize)]
struct GoogleDriveListResponse {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    #[serde(default)]
    files: Vec<GoogleDriveFile>,
}

#[derive(Debug, Deserialize)]
struct GoogleDriveFile {
    id: String,
    #[serde(rename = "appProperties")]
    app_properties: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct GoogleDriveCreateResponse {
    id: String,
}

pub struct DropboxBackend {
    client: HttpClient,
    auth: OAuthAccessState,
    root_path: String,
    root_path_lower: String,
    throttle: RequestThrottle,
}

impl DropboxBackend {
    #[allow(dead_code)]
    pub fn from_remote(remote: Option<&str>) -> Result<Self, String> {
        let credentials = OAuthCredentialBundle::from_env(
            "BSJ_DROPBOX_ACCESS_TOKEN",
            "BSJ_DROPBOX_REFRESH_TOKEN",
            "BSJ_DROPBOX_APP_KEY",
            "BSJ_DROPBOX_APP_SECRET",
        );
        Self::from_remote_with_credentials(remote, credentials)
    }

    pub fn from_remote_with_credentials(
        remote: Option<&str>,
        credentials: OAuthCredentialBundle,
    ) -> Result<Self, String> {
        let client = HttpClient::builder()
            .build()
            .map_err(|error| format!("failed to create Dropbox client: {error}"))?;
        let mut auth = OAuthAccessState::from_credentials(
            "Dropbox",
            credentials,
            "BSJ_DROPBOX_ACCESS_TOKEN",
            "BSJ_DROPBOX_REFRESH_TOKEN",
            "BSJ_DROPBOX_APP_KEY",
            "BSJ_DROPBOX_APP_SECRET",
        )?;
        if auth.token().trim().is_empty() {
            auth.refresh(&client, DROPBOX_TOKEN_URL)
                .map_err(|error| format!("Dropbox token refresh failed: {error}"))?;
        }
        let root_path = parse_dropbox_root(remote)?;
        let root_path_lower = root_path.to_ascii_lowercase();
        Ok(Self {
            client,
            auth,
            root_path,
            root_path_lower,
            throttle: RequestThrottle::default(),
        })
    }

    fn with_auth<F>(
        &mut self,
        label: &str,
        mut build: F,
    ) -> VaultResult<reqwest::blocking::Response>
    where
        F: FnMut(&HttpClient, &str) -> RequestBuilder,
    {
        let mut refreshed = false;
        loop {
            self.throttle.wait_for_turn();
            let response = build(&self.client, self.auth.token())
                .send()
                .map_err(|error| {
                    VaultError::Sync(format!("Dropbox {label} request failed: {error}"))
                })?;

            if response.status() == StatusCode::UNAUTHORIZED
                && !refreshed
                && self.auth.can_refresh()
            {
                self.auth.refresh(&self.client, DROPBOX_TOKEN_URL)?;
                refreshed = true;
                continue;
            }

            return Ok(response);
        }
    }

    fn ensure_root_exists(&mut self) -> VaultResult<()> {
        if self.root_path == "/" {
            return Ok(());
        }
        let segments = self
            .root_path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut current = String::new();
        for segment in segments {
            current.push('/');
            current.push_str(&segment);
            let current_path = current.clone();
            let response = self.with_auth("create-folder", |client, token| {
                client
                    .post(format!("{DROPBOX_API_BASE}/files/create_folder_v2"))
                    .bearer_auth(token)
                    .json(&serde_json::json!({
                        "path": current_path,
                        "autorename": false,
                    }))
            })?;
            if response.status().is_success() {
                continue;
            }
            if response.status() == StatusCode::CONFLICT {
                continue;
            }
            let body = response.text().unwrap_or_default().to_ascii_lowercase();
            if body.contains("conflict") {
                continue;
            }
            return Err(VaultError::Sync(format!(
                "Dropbox create folder failed for {}: {}",
                current, body
            )));
        }
        Ok(())
    }

    fn list_keys_once(&mut self) -> VaultResult<BTreeSet<String>> {
        let mut keys = BTreeSet::new();
        let root_path = self.root_path.clone();
        let response = self.with_auth("list-folder", |client, token| {
            client
                .post(format!("{DROPBOX_API_BASE}/files/list_folder"))
                .bearer_auth(token)
                .json(&serde_json::json!({
                    "path": root_path,
                    "recursive": true,
                    "include_deleted": false,
                    "limit": 2000,
                }))
        })?;

        if !response.status().is_success() {
            if response.status() == StatusCode::CONFLICT {
                return Ok(keys);
            }
            return Err(response_to_sync_error("Dropbox", "list folder", response));
        }

        let mut payload: DropboxListResponse = response
            .json()
            .map_err(|error| VaultError::Sync(format!("Dropbox list parse failed: {error}")))?;
        collect_dropbox_keys(&payload.entries, &self.root_path_lower, &mut keys);

        while payload.has_more {
            let cursor = payload.cursor.clone();
            let response = self.with_auth("list-folder-continue", |client, token| {
                client
                    .post(format!("{DROPBOX_API_BASE}/files/list_folder/continue"))
                    .bearer_auth(token)
                    .json(&serde_json::json!({ "cursor": cursor }))
            })?;
            if !response.status().is_success() {
                return Err(response_to_sync_error("Dropbox", "list continue", response));
            }
            payload = response.json().map_err(|error| {
                VaultError::Sync(format!("Dropbox list continue parse failed: {error}"))
            })?;
            collect_dropbox_keys(&payload.entries, &self.root_path_lower, &mut keys);
        }

        Ok(keys)
    }

    fn read_once(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        let path = dropbox_object_path(&self.root_path, key);
        let response = self.with_auth("download", |client, token| {
            client
                .post(format!("{DROPBOX_CONTENT_BASE}/files/download"))
                .bearer_auth(token)
                .header(
                    "Dropbox-API-Arg",
                    serde_json::json!({ "path": path }).to_string(),
                )
        })?;
        if !response.status().is_success() {
            return Err(response_to_sync_error("Dropbox", "download", response));
        }
        response
            .bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(|error| VaultError::Sync(format!("Dropbox read failed: {error}")))
    }

    fn write_once(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        self.ensure_root_exists()?;
        let path = dropbox_object_path(&self.root_path, key);
        let response = self.with_auth("upload", |client, token| {
            client
                .post(format!("{DROPBOX_CONTENT_BASE}/files/upload"))
                .bearer_auth(token)
                .header(
                    "Dropbox-API-Arg",
                    serde_json::json!({
                        "path": path,
                        "mode": "overwrite",
                        "autorename": false,
                        "mute": true,
                    })
                    .to_string(),
                )
                .header("Content-Type", "application/octet-stream")
                .body(bytes.to_vec())
        })?;
        if !response.status().is_success() {
            return Err(response_to_sync_error("Dropbox", "upload", response));
        }
        Ok(())
    }
}

impl SyncBackend for DropboxBackend {
    fn list_keys(&mut self) -> VaultResult<BTreeSet<String>> {
        retry_sync_operation("Dropbox list", || self.list_keys_once())
    }

    fn read(&mut self, key: &str) -> VaultResult<Vec<u8>> {
        retry_sync_operation(&format!("Dropbox read {key}"), || self.read_once(key))
    }

    fn write(&mut self, key: &str, bytes: &[u8]) -> VaultResult<()> {
        retry_sync_operation(&format!("Dropbox write {key}"), || {
            self.write_once(key, bytes)
        })
    }
}

#[derive(Debug, Deserialize)]
struct DropboxListResponse {
    #[serde(default)]
    entries: Vec<DropboxEntry>,
    cursor: String,
    #[serde(rename = "has_more")]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct DropboxEntry {
    #[serde(rename = ".tag")]
    tag: String,
    #[serde(default)]
    path_lower: Option<String>,
}

fn collect_dropbox_keys(entries: &[DropboxEntry], root_lower: &str, keys: &mut BTreeSet<String>) {
    for entry in entries {
        if entry.tag != "file" {
            continue;
        }
        let Some(path_lower) = entry.path_lower.as_deref() else {
            continue;
        };
        let Some(relative) = dropbox_relative_key(path_lower, root_lower) else {
            continue;
        };
        if classify_sync_key(&relative).is_some() {
            keys.insert(relative);
        }
    }
}

fn dropbox_relative_key(path_lower: &str, root_lower: &str) -> Option<String> {
    if root_lower == "/" {
        return path_lower.strip_prefix('/').map(ToString::to_string);
    }
    path_lower
        .strip_prefix(root_lower)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .map(ToString::to_string)
}

fn response_to_sync_error(
    provider: &str,
    label: &str,
    response: reqwest::blocking::Response,
) -> VaultError {
    let status = response.status();
    let mut body = response.text().unwrap_or_default().trim().to_string();
    if body.len() > 220 {
        body.truncate(220);
        body.push_str("...");
    }
    let message = if body.is_empty() {
        format!("{provider} {label} failed: {status}")
    } else {
        format!("{provider} {label} failed: {status} {body}")
    };
    VaultError::Sync(message)
}

fn google_drive_object_name(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    format!("bsj-{}.bin", hex::encode(digest))
}

fn google_drive_multipart_payload(boundary: &str, metadata_json: &str, bytes: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(metadata_json.len() + bytes.len() + 256);
    payload.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata_json}\r\n"
        )
        .as_bytes(),
    );
    payload.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes(),
    );
    payload.extend_from_slice(bytes);
    payload.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    payload
}

fn parse_google_drive_remote(remote: Option<&str>) -> Result<GoogleDriveRoot, String> {
    let raw = match remote {
        Some(remote) => remote.trim().to_string(),
        None => env::var("BSJ_GDRIVE_FOLDER_ID").unwrap_or_default(),
    };
    if raw.is_empty() || raw.eq_ignore_ascii_case("appdata") {
        return Ok(GoogleDriveRoot::AppDataFolder);
    }

    if let Some(rest) = raw.strip_prefix("gdrive://") {
        return parse_google_drive_remote(Some(rest));
    }
    if let Some(rest) = raw.strip_prefix("googledrive://") {
        return parse_google_drive_remote(Some(rest));
    }
    if let Some(folder_id) = parse_google_folder_id_from_url(&raw) {
        return Ok(GoogleDriveRoot::FolderId(folder_id));
    }
    Ok(GoogleDriveRoot::FolderId(raw))
}

fn parse_google_folder_id_from_url(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if !host.contains("drive.google.com") {
        return None;
    }
    let path = url.path();
    let marker = "/folders/";
    let index = path.find(marker)?;
    let id = &path[index + marker.len()..];
    let folder_id = id.split('/').next().unwrap_or_default().trim();
    (!folder_id.is_empty()).then(|| folder_id.to_string())
}

fn parse_dropbox_root(remote: Option<&str>) -> Result<String, String> {
    let raw = match remote {
        Some(remote) => remote.trim().to_string(),
        None => env::var("BSJ_DROPBOX_ROOT").unwrap_or_else(|_| DROPBOX_DEFAULT_ROOT.to_string()),
    };
    let trimmed = if let Some(path) = raw.strip_prefix("dropbox://") {
        path.trim()
    } else {
        raw.trim()
    };
    let normalized = if trimmed.is_empty() {
        DROPBOX_DEFAULT_ROOT.to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    if normalized.contains('\\') {
        return Err("Dropbox root must use forward slashes".to_string());
    }
    if normalized == "/" {
        Ok("/".to_string())
    } else {
        Ok(normalized.trim_end_matches('/').to_string())
    }
}

fn dropbox_object_path(root: &str, key: &str) -> String {
    let root = if root == "/" {
        "".to_string()
    } else {
        root.trim_end_matches('/').to_string()
    };
    if root.is_empty() {
        format!("/{key}")
    } else {
        format!("{root}/{key}")
    }
}

fn normalize_secret_option(value: Option<SecretString>) -> Option<SecretString> {
    value.filter(|secret| !secret.expose_secret().trim().is_empty())
}

fn secret_option_is_present(value: Option<&SecretString>) -> bool {
    value
        .map(|secret| !secret.expose_secret().trim().is_empty())
        .unwrap_or(false)
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

pub fn recover_root<B: SyncBackend>(
    local_root: &Path,
    backend: &mut B,
) -> VaultResult<BackendSyncReport> {
    let remote_keys = backend.list_keys()?;
    if !remote_keys.contains("vault.json") {
        return Err(VaultError::Sync(
            "remote recovery source is missing vault.json".to_string(),
        ));
    }

    secure_fs::ensure_private_dir(local_root)?;
    secure_fs::ensure_private_dir(&local_root.join("entries"))?;
    secure_fs::ensure_private_dir(&local_root.join("devices"))?;

    let mut pulled = 0usize;
    for key in &remote_keys {
        let local_path = sync_key_to_path(local_root, key)?;
        if local_path.exists() {
            continue;
        }
        let bytes = backend.read(key)?;
        write_local_key(local_root, key, &bytes)?;
        if matches!(classify_sync_key(key), Some(SyncObjectKind::Revision)) {
            pulled += 1;
        }
    }

    ensure_local_and_remote_metadata_match(local_root, backend)?;
    let local_inventory = list_local_inventory(local_root)?;
    ensure_shared_revision_bytes_match(local_root, backend, &local_inventory, &remote_keys)?;

    Ok(BackendSyncReport { pulled, pushed: 0 })
}

pub fn preview_root<B: SyncBackend>(
    _metadata: &VaultMetadata,
    local_root: &Path,
    backend: &mut B,
) -> VaultResult<SyncPreviewReport> {
    let local_inventory = list_local_inventory(local_root)?;
    let remote_keys = backend.list_keys()?;
    let local_revisions = local_inventory.revision_keys.len();
    let remote_revisions = remote_keys
        .iter()
        .filter(|key| classify_sync_key(key) == Some(SyncObjectKind::Revision))
        .count();
    let local_only_revisions = local_inventory
        .revision_keys
        .iter()
        .filter(|key| !remote_keys.contains(*key))
        .count();
    let remote_only_revisions = remote_keys
        .iter()
        .filter(|key| {
            classify_sync_key(key) == Some(SyncObjectKind::Revision)
                && !local_inventory.revision_keys.contains(*key)
        })
        .count();
    let shared_revisions = local_inventory
        .revision_keys
        .intersection(
            &remote_keys
                .iter()
                .filter(|key| classify_sync_key(key) == Some(SyncObjectKind::Revision))
                .cloned()
                .collect(),
        )
        .count();
    Ok(SyncPreviewReport {
        local_revisions,
        remote_revisions,
        local_only_revisions,
        remote_only_revisions,
        shared_revisions,
    })
}

pub fn looks_like_s3_remote(remote: &str) -> bool {
    remote.starts_with("s3://")
}

pub fn looks_like_google_drive_remote(remote: &str) -> bool {
    remote.starts_with("gdrive://")
        || remote.starts_with("googledrive://")
        || remote.contains("drive.google.com/drive/folders/")
}

pub fn looks_like_dropbox_remote(remote: &str) -> bool {
    remote.starts_with("dropbox://")
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

fn ensure_local_and_remote_metadata_match<B: SyncBackend>(
    local_root: &Path,
    backend: &mut B,
) -> VaultResult<()> {
    let local_vault_json = fs::read(local_root.join("vault.json"))?;
    let remote_vault_json = backend.read("vault.json")?;
    let local_metadata: VaultMetadata = serde_json::from_slice(&local_vault_json)?;
    let remote_metadata: VaultMetadata = serde_json::from_slice(&remote_vault_json)?;
    if !vault_metadata_compatible(&local_metadata, &remote_metadata) {
        return Err(VaultError::InvalidFormat(
            "recovery source metadata is incompatible".to_string(),
        ));
    }
    Ok(())
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
    ensure_local_sync_parent_dirs(root, key)?;
    atomic_write(&sync_key_to_path(root, key)?, bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> VaultResult<()> {
    secure_fs::atomic_write_private(path, bytes).map_err(Into::into)
}

fn ensure_local_sync_parent_dirs(root: &Path, key: &str) -> VaultResult<()> {
    if key == "vault.json" {
        return Ok(());
    }

    if key.starts_with("devices/") {
        secure_fs::ensure_private_dir(&root.join("devices"))?;
        return Ok(());
    }

    let parts = key.split('/').collect::<Vec<_>>();
    if parts.len() == 4 && parts[0] == "entries" {
        let entries_root = root.join("entries");
        secure_fs::ensure_private_dir(&entries_root)?;
        let year_dir = entries_root.join(parts[1]);
        secure_fs::ensure_private_dir(&year_dir)?;
        secure_fs::ensure_private_dir(&year_dir.join(parts[2]))?;
    }

    Ok(())
}

fn insecure_webdav_http_allowed() -> bool {
    matches!(
        env::var("BSJ_WEBDAV_ALLOW_INSECURE_HTTP")
            .ok()
            .map(|value| value.to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes")
    )
}

fn validate_webdav_base_url(raw_url: &str, allow_insecure_http: bool) -> Result<Url, String> {
    let base_url = Url::parse(raw_url).map_err(|error| format!("invalid WebDAV URL: {error}"))?;
    match base_url.scheme() {
        "https" => Ok(base_url),
        "http" if allow_insecure_http => Ok(base_url),
        "http" => Err(
            "WebDAV URL must use https:// by default. Set BSJ_WEBDAV_ALLOW_INSECURE_HTTP=1 only for trusted testing."
                .to_string(),
        ),
        _ => Err("WebDAV URL must use http:// or https://".to_string()),
    }
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
    fn recover_root_restores_missing_local_vault_from_remote() {
        let dir = tempdir().expect("tempdir");
        let local_root = dir.path().join("local-recovered");
        let remote_root = dir.path().join("remote-source");
        let passphrase = SecretString::new("correct horse battery staple".into());

        create_vault(&remote_root, &passphrase, None, "Remote").expect("create remote");
        let remote =
            unlock_vault_with_device(&remote_root, &passphrase, "device-remote").expect("unlock");
        register_device(&remote_root, "device-remote", "Remote").expect("register");
        remote
            .save_revision(
                NaiveDate::from_ymd_opt(2026, 3, 18).expect("date"),
                "remote-only entry",
            )
            .expect("save");

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

        let report = recover_root(&local_root, &mut backend).expect("recover");
        assert_eq!(report.pulled, 1);
        assert_eq!(report.pushed, 0);
        assert!(local_root.join("vault.json").exists());
        assert!(
            local_root
                .join("entries/2026/2026-03-18/rev-device-remote-000001.bsj.enc")
                .exists()
        );
    }

    #[test]
    fn recover_root_requires_remote_vault_json() {
        let dir = tempdir().expect("tempdir");
        let local_root = dir.path().join("local");
        let mut backend = MemoryBackend::default();
        backend.insert(
            "entries/2026/2026-03-18/rev-device-a-000001.bsj.enc",
            vec![1, 2, 3],
        );

        let error = recover_root(&local_root, &mut backend).expect_err("missing metadata");
        assert!(error.to_string().contains("missing vault.json"));
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
    fn gdrive_backend_smoke_lists_when_env_present() {
        if env::var("BSJ_GDRIVE_ACCESS_TOKEN").is_err()
            && env::var("BSJ_GDRIVE_REFRESH_TOKEN").is_err()
        {
            eprintln!(
                "skipping Google Drive integration smoke test: BSJ_GDRIVE_ACCESS_TOKEN or BSJ_GDRIVE_REFRESH_TOKEN not set"
            );
            return;
        }
        let mut backend = GoogleDriveBackend::from_remote(None).expect("backend");
        backend.list_keys().expect("list");
    }

    #[test]
    fn dropbox_backend_smoke_lists_when_env_present() {
        if env::var("BSJ_DROPBOX_ACCESS_TOKEN").is_err()
            && env::var("BSJ_DROPBOX_REFRESH_TOKEN").is_err()
        {
            eprintln!(
                "skipping Dropbox integration smoke test: BSJ_DROPBOX_ACCESS_TOKEN or BSJ_DROPBOX_REFRESH_TOKEN not set"
            );
            return;
        }
        let mut backend = DropboxBackend::from_remote(None).expect("backend");
        backend.list_keys().expect("list");
    }

    #[test]
    fn webdav_url_detection_matches_http_schemes() {
        assert!(looks_like_webdav_remote("https://dav.example.com/bsj/"));
        assert!(looks_like_webdav_remote("http://dav.example.com/bsj/"));
        assert!(!looks_like_webdav_remote("/tmp/bsj-sync"));
    }

    #[test]
    fn s3_url_detection_matches_s3_scheme() {
        assert!(looks_like_s3_remote("s3://bucket/prefix"));
        assert!(!looks_like_s3_remote("https://example.com/bucket"));
    }

    #[test]
    fn gdrive_url_detection_matches_supported_forms() {
        assert!(looks_like_google_drive_remote("gdrive://appdata"));
        assert!(looks_like_google_drive_remote("googledrive://folder-id"));
        assert!(looks_like_google_drive_remote(
            "https://drive.google.com/drive/folders/abc123"
        ));
        assert!(!looks_like_google_drive_remote(
            "https://example.com/drive/folders/abc123"
        ));
    }

    #[test]
    fn dropbox_url_detection_matches_dropbox_scheme() {
        assert!(looks_like_dropbox_remote(
            "dropbox:///BlueScreenJournal-Sync"
        ));
        assert!(!looks_like_dropbox_remote("https://www.dropbox.com/home"));
    }

    #[test]
    fn parse_google_drive_remote_supports_appdata_and_urls() {
        assert_eq!(
            parse_google_drive_remote(Some("appdata")).expect("appdata"),
            GoogleDriveRoot::AppDataFolder
        );
        assert_eq!(
            parse_google_drive_remote(Some("gdrive://folder-123")).expect("scheme"),
            GoogleDriveRoot::FolderId("folder-123".to_string())
        );
        assert_eq!(
            parse_google_drive_remote(Some("https://drive.google.com/drive/folders/folder-abc"))
                .expect("url"),
            GoogleDriveRoot::FolderId("folder-abc".to_string())
        );
    }

    #[test]
    fn parse_dropbox_root_normalizes_paths() {
        assert_eq!(
            parse_dropbox_root(Some("dropbox://journal-sync")).expect("scheme"),
            "/journal-sync"
        );
        assert_eq!(
            parse_dropbox_root(Some("/journal-sync/")).expect("trim"),
            "/journal-sync"
        );
        assert_eq!(parse_dropbox_root(Some("/")).expect("root"), "/");
    }

    #[test]
    fn webdav_constructor_normalizes_base_url() {
        let backend =
            WebDavBackend::from_remote(Some("https://example.com/bsj-test")).expect("backend");
        assert_eq!(backend.base_url.as_str(), "https://example.com/bsj-test/");
    }

    #[test]
    fn insecure_webdav_http_is_rejected_by_default() {
        let error = validate_webdav_base_url("http://example.com/bsj/", false).expect_err("http");
        assert!(error.contains("https://"));
    }

    #[test]
    fn insecure_webdav_http_can_be_explicitly_allowed() {
        let url = validate_webdav_base_url("http://example.com/bsj/", true).expect("allow http");
        assert_eq!(url.as_str(), "http://example.com/bsj/");
    }

    #[test]
    fn retry_sync_operation_retries_transient_sync_errors() {
        let mut attempts = 0usize;
        let value = retry_sync_operation("test retry", || {
            attempts += 1;
            if attempts == 1 {
                Err(VaultError::Sync("try again".to_string()))
            } else {
                Ok(attempts)
            }
        })
        .expect("retry succeeds");

        assert_eq!(value, 2);
    }

    #[test]
    fn retry_sync_operation_does_not_retry_invalid_format_errors() {
        let mut attempts = 0usize;
        let error = retry_sync_operation("test invalid", || {
            attempts += 1;
            Err::<(), _>(VaultError::InvalidFormat("bad".to_string()))
        })
        .expect_err("no retry");

        assert_eq!(attempts, 1);
        assert!(error.to_string().contains("bad"));
    }

    #[test]
    fn oauth_bundle_fills_missing_env_fields_from_keychain_bundle() {
        let env_bundle = OAuthCredentialBundle {
            access_token: Some(SecretString::new("env-access".to_string().into_boxed_str())),
            refresh_token: None,
            client_id: None,
            client_secret: None,
        };
        let keychain_bundle = OAuthCredentialBundle {
            access_token: None,
            refresh_token: Some(SecretString::new("refresh".to_string().into_boxed_str())),
            client_id: Some(SecretString::new("client".to_string().into_boxed_str())),
            client_secret: Some(SecretString::new("secret".to_string().into_boxed_str())),
        };

        let merged = env_bundle.fill_missing_from(keychain_bundle);
        let presence = merged.presence();
        assert!(presence.access_token);
        assert!(presence.refresh_token);
        assert!(presence.client_id);
        assert!(presence.client_secret);
        assert!(merged.ready());
    }

    #[test]
    fn oauth_presence_source_label_reflects_env_and_keychain() {
        let env_presence = OAuthCredentialPresence {
            access_token: true,
            ..OAuthCredentialPresence::default()
        };
        let keychain_presence = OAuthCredentialPresence {
            refresh_token: true,
            client_id: true,
            client_secret: true,
            ..OAuthCredentialPresence::default()
        };

        assert_eq!(env_presence.source_label(keychain_presence), "ENV+KEYCHAIN");
        assert_eq!(
            OAuthCredentialPresence::default().source_label(keychain_presence),
            "KEYCHAIN"
        );
    }
}
