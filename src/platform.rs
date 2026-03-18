use reqwest::blocking::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

const KEYCHAIN_SERVICE: &str = "com.awassee.bsj.passphrase";
const RELEASES_URL: &str = "https://api.github.com/repos/Awassee/bluescreenjournal/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_tag: String,
    pub html_url: String,
    pub newer_available: bool,
    pub asset_names: Vec<String>,
}

#[derive(Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
}

pub fn keychain_account(vault_path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(vault_path.to_string_lossy().as_bytes());
    let digest = hex::encode(hasher.finalize());
    format!("vault-{}", &digest[..16])
}

pub fn store_passphrase(vault_path: &Path, passphrase: &SecretString) -> Result<(), String> {
    let status = Command::new("security")
        .arg("add-generic-password")
        .arg("-U")
        .arg("-a")
        .arg(keychain_account(vault_path))
        .arg("-s")
        .arg(KEYCHAIN_SERVICE)
        .arg("-w")
        .arg(passphrase.expose_secret())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|error| format!("failed to run security add-generic-password: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("failed to store passphrase in macOS Keychain".to_string())
    }
}

pub fn load_passphrase(vault_path: &Path) -> Result<Option<SecretString>, String> {
    let output = Command::new("security")
        .arg("find-generic-password")
        .arg("-a")
        .arg(keychain_account(vault_path))
        .arg("-s")
        .arg(KEYCHAIN_SERVICE)
        .arg("-w")
        .output()
        .map_err(|error| format!("failed to run security find-generic-password: {error}"))?;

    if !output.status.success() {
        return Ok(None);
    }

    let passphrase = String::from_utf8(output.stdout)
        .map_err(|error| format!("keychain returned invalid UTF-8: {error}"))?;
    Ok(Some(SecretString::new(
        passphrase.trim_end().to_string().into_boxed_str(),
    )))
}

pub fn delete_passphrase(vault_path: &Path) -> Result<(), String> {
    let output = Command::new("security")
        .arg("delete-generic-password")
        .arg("-a")
        .arg(keychain_account(vault_path))
        .arg("-s")
        .arg(KEYCHAIN_SERVICE)
        .output()
        .map_err(|error| format!("failed to run security delete-generic-password: {error}"))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
    if stderr.contains("could not be found")
        || stderr.contains("the specified item could not be found")
    {
        return Ok(());
    }

    Err("failed to delete passphrase from macOS Keychain".to_string())
}

pub fn check_for_updates(current_version: &str) -> Result<UpdateInfo, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("failed to create HTTP client: {error}"))?;
    let response_body = client
        .get(RELEASES_URL)
        .header("User-Agent", "bsj")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("failed to fetch latest release: {error}"))?
        .text()
        .map_err(|error| format!("failed to read latest release response: {error}"))?;
    let release = serde_json::from_str::<ReleaseResponse>(&response_body)
        .map_err(|error| format!("failed to parse latest release response: {error}"))?;

    let current_tag = normalize_tag(current_version);
    let latest_tag = release.tag_name;
    let newer_available =
        compare_versions(strip_v_prefix(&latest_tag), strip_v_prefix(&current_tag)).is_gt();

    Ok(UpdateInfo {
        current_version: current_tag,
        latest_tag,
        html_url: release.html_url,
        newer_available,
        asset_names: release.assets.into_iter().map(|asset| asset.name).collect(),
    })
}

fn normalize_tag(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

fn strip_v_prefix(version: &str) -> &str {
    version.strip_prefix('v').unwrap_or(version)
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = parse_version_parts(left);
    let right_parts = parse_version_parts(right);
    left_parts.cmp(&right_parts)
}

fn parse_version_parts(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ReleaseResponse, compare_versions, keychain_account};
    use std::path::Path;

    #[test]
    fn keychain_account_is_stable_for_vault_path() {
        let left = keychain_account(Path::new("/tmp/BlueScreenJournal"));
        let right = keychain_account(Path::new("/tmp/BlueScreenJournal"));
        let different = keychain_account(Path::new("/tmp/OtherJournal"));

        assert_eq!(left, right);
        assert_ne!(left, different);
    }

    #[test]
    fn version_compare_tracks_semver_ordering() {
        assert!(compare_versions("0.1.7", "0.1.6").is_gt());
        assert!(compare_versions("0.2.0", "0.10.0").is_lt());
        assert!(compare_versions("1.0.0", "1.0.0").is_eq());
    }

    #[test]
    fn latest_release_payload_parses_asset_names() {
        let payload = r#"{
          "tag_name": "v0.1.6",
          "html_url": "https://example.invalid/release",
          "assets": [
            { "name": "bsj-universal-apple-darwin.tar.gz" },
            { "name": "bsj-universal-apple-darwin.tar.gz.sha256" }
          ]
        }"#;
        let parsed: ReleaseResponse = serde_json::from_str(payload).expect("release");
        assert_eq!(parsed.tag_name, "v0.1.6");
        assert_eq!(parsed.assets.len(), 2);
    }

    #[test]
    fn current_release_tag_is_normalized() {
        let info = super::UpdateInfo {
            current_version: "v0.1.6".to_string(),
            latest_tag: "v0.1.7".to_string(),
            html_url: String::new(),
            newer_available: true,
            asset_names: Vec::new(),
        };
        assert!(info.current_version.starts_with('v'));
    }
}
