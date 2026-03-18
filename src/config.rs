use crate::secure_fs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use thiserror::Error;

const READABLE_SETTING_KEYS: &[&str] = &[
    "vault_path",
    "sync_target_path",
    "device_nickname",
    "typewriter_mode",
    "daily_word_goal",
    "remember_passphrase_in_keychain",
    "backup_retention.daily",
    "backup_retention.weekly",
    "backup_retention.monthly",
    "local_device_id",
];

const EDITABLE_SETTING_KEYS: &[&str] = &[
    "vault_path",
    "sync_target_path",
    "device_nickname",
    "typewriter_mode",
    "daily_word_goal",
    "remember_passphrase_in_keychain",
    "backup_retention.daily",
    "backup_retention.weekly",
    "backup_retention.monthly",
];

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to determine config directory")]
    MissingConfigDir,
    #[error("failed to parse config: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("failed to read config: {0}")]
    Read(#[from] std::io::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_vault_path")]
    pub vault_path: PathBuf,
    #[serde(default)]
    pub sync_target_path: Option<PathBuf>,
    #[serde(default)]
    pub local_device_id: Option<String>,
    #[serde(default = "default_device_nickname")]
    pub device_nickname: String,
    #[serde(default)]
    pub typewriter_mode: bool,
    #[serde(default)]
    pub daily_word_goal: Option<usize>,
    #[serde(default)]
    pub remember_passphrase_in_keychain: bool,
    #[serde(default)]
    pub first_run_coach_completed: bool,
    #[serde(default)]
    pub last_sync: Option<LastSyncInfo>,
    #[serde(default)]
    pub sync_history: Vec<LastSyncInfo>,
    #[serde(default)]
    pub favorite_dates: Vec<String>,
    #[serde(default)]
    pub backup_retention: BackupRetentionConfig,
    #[serde(default)]
    pub macros: Vec<MacroConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LastSyncInfo {
    pub timestamp: String,
    pub backend: String,
    pub target: String,
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: usize,
    pub integrity_ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupRetentionConfig {
    #[serde(default = "default_daily_backups")]
    pub daily: usize,
    #[serde(default = "default_weekly_backups")]
    pub weekly: usize,
    #[serde(default = "default_monthly_backups")]
    pub monthly: usize,
}

impl Default for BackupRetentionConfig {
    fn default() -> Self {
        Self {
            daily: default_daily_backups(),
            weekly: default_weekly_backups(),
            monthly: default_monthly_backups(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MacroConfig {
    pub key: String,
    #[serde(flatten)]
    pub action: MacroActionConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MacroActionConfig {
    InsertTemplate { text: String },
    Command { command: MacroCommandConfig },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroCommandConfig {
    InsertDateHeader,
    InsertClosingLine,
    JumpToday,
}

impl AppConfig {
    pub fn load() -> Result<Option<Self>, ConfigError> {
        let path = config_file_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(path)?;
        let parsed = serde_json::from_slice::<Self>(&bytes)?;
        Ok(Some(parsed))
    }

    pub fn load_or_default() -> Self {
        match Self::load() {
            Ok(Some(config)) => config,
            Ok(None) | Err(_) => Self {
                vault_path: default_vault_path(),
                sync_target_path: None,
                local_device_id: None,
                device_nickname: default_device_nickname(),
                typewriter_mode: false,
                daily_word_goal: None,
                remember_passphrase_in_keychain: false,
                first_run_coach_completed: false,
                last_sync: None,
                sync_history: Vec::new(),
                favorite_dates: Vec::new(),
                backup_retention: BackupRetentionConfig::default(),
                macros: Vec::new(),
            },
        }
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_file_path()?;
        let bytes = serde_json::to_vec_pretty(self)?;
        secure_fs::atomic_write_private(&path, &bytes)?;
        Ok(())
    }
}

pub fn default_vault_path() -> PathBuf {
    if let Some(documents_dir) = dirs::document_dir() {
        return documents_dir.join("BlueScreenJournal");
    }

    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("Documents").join("BlueScreenJournal")
}

pub fn config_file_path() -> Result<PathBuf, ConfigError> {
    let dir = dirs::config_dir().ok_or(ConfigError::MissingConfigDir)?;
    Ok(dir.join("bsj").join("config.json"))
}

pub fn readable_setting_keys() -> &'static [&'static str] {
    READABLE_SETTING_KEYS
}

pub fn editable_setting_keys() -> &'static [&'static str] {
    EDITABLE_SETTING_KEYS
}

pub fn get_setting_value(config: &AppConfig, key: &str) -> Result<String, String> {
    match key {
        "vault_path" => Ok(config.vault_path.display().to_string()),
        "sync_target_path" => Ok(config
            .sync_target_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "null".to_string())),
        "device_nickname" => Ok(config.device_nickname.clone()),
        "typewriter_mode" => Ok(config.typewriter_mode.to_string()),
        "daily_word_goal" => Ok(config
            .daily_word_goal
            .map(|goal| goal.to_string())
            .unwrap_or_else(|| "null".to_string())),
        "remember_passphrase_in_keychain" => Ok(config.remember_passphrase_in_keychain.to_string()),
        "backup_retention.daily" => Ok(config.backup_retention.daily.to_string()),
        "backup_retention.weekly" => Ok(config.backup_retention.weekly.to_string()),
        "backup_retention.monthly" => Ok(config.backup_retention.monthly.to_string()),
        "local_device_id" => Ok(config
            .local_device_id
            .clone()
            .unwrap_or_else(|| "null".to_string())),
        _ => Err(format!(
            "unknown setting '{key}'. Known keys: {}",
            READABLE_SETTING_KEYS.join(", ")
        )),
    }
}

pub fn set_setting_value(config: &mut AppConfig, key: &str, value: &str) -> Result<String, String> {
    match key {
        "vault_path" => {
            config.vault_path = expand_path_like(value);
            Ok(config.vault_path.display().to_string())
        }
        "sync_target_path" => {
            config.sync_target_path = normalize_optional_path(value);
            Ok(config
                .sync_target_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "null".to_string()))
        }
        "device_nickname" => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err("device_nickname cannot be empty".to_string());
            }
            config.device_nickname = trimmed.to_string();
            Ok(config.device_nickname.clone())
        }
        "typewriter_mode" => {
            config.typewriter_mode = parse_bool_value(value, key)?;
            Ok(config.typewriter_mode.to_string())
        }
        "daily_word_goal" => {
            config.daily_word_goal = parse_optional_usize(value, key)?;
            Ok(config
                .daily_word_goal
                .map(|goal| goal.to_string())
                .unwrap_or_else(|| "null".to_string()))
        }
        "remember_passphrase_in_keychain" => {
            config.remember_passphrase_in_keychain = parse_bool_value(value, key)?;
            Ok(config.remember_passphrase_in_keychain.to_string())
        }
        "backup_retention.daily" => {
            config.backup_retention.daily = parse_retention_value(value, key)?;
            Ok(config.backup_retention.daily.to_string())
        }
        "backup_retention.weekly" => {
            config.backup_retention.weekly = parse_retention_value(value, key)?;
            Ok(config.backup_retention.weekly.to_string())
        }
        "backup_retention.monthly" => {
            config.backup_retention.monthly = parse_retention_value(value, key)?;
            Ok(config.backup_retention.monthly.to_string())
        }
        "local_device_id" => {
            Err("local_device_id is app-managed and cannot be set manually".to_string())
        }
        _ => Err(format!(
            "unknown setting '{key}'. Editable keys: {}",
            EDITABLE_SETTING_KEYS.join(", ")
        )),
    }
}

pub fn expand_path_like(input: &str) -> PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(input));
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(input)
}

fn normalize_optional_path(value: &str) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("null")
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("unset")
    {
        None
    } else {
        Some(expand_path_like(trimmed))
    }
}

fn parse_retention_value(value: &str, key: &str) -> Result<usize, String> {
    value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("{key} must be a non-negative integer"))
}

fn parse_bool_value(value: &str, key: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("{key} must be true or false")),
    }
}

fn parse_optional_usize(value: &str, key: &str) -> Result<Option<usize>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("null")
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("unset")
    {
        return Ok(None);
    }
    trimmed
        .parse::<usize>()
        .map(Some)
        .map_err(|_| format!("{key} must be blank or a non-negative integer"))
}

fn default_daily_backups() -> usize {
    7
}

fn default_weekly_backups() -> usize {
    4
}

fn default_monthly_backups() -> usize {
    6
}

fn default_device_nickname() -> String {
    "This Mac".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, BackupRetentionConfig, default_vault_path, get_setting_value, set_setting_value,
    };
    use std::path::PathBuf;

    #[test]
    fn default_vault_path_targets_documents_bluescreenjournal() {
        let path = default_vault_path();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("BlueScreenJournal")
        );
        assert!(
            path.components()
                .any(|component| component.as_os_str() == "Documents")
        );
    }

    #[test]
    fn get_setting_value_reports_known_keys() {
        let config = AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: Some(PathBuf::from("/tmp/remote")),
            local_device_id: Some("device".to_string()),
            device_nickname: "QA Mac".to_string(),
            typewriter_mode: true,
            daily_word_goal: Some(750),
            remember_passphrase_in_keychain: true,
            first_run_coach_completed: true,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: vec!["2026-03-17".to_string()],
            backup_retention: BackupRetentionConfig {
                daily: 5,
                weekly: 4,
                monthly: 3,
            },
            macros: Vec::new(),
        };

        assert_eq!(
            get_setting_value(&config, "backup_retention.daily").expect("daily"),
            "5"
        );
        assert_eq!(
            get_setting_value(&config, "local_device_id").expect("device id"),
            "device"
        );
        assert_eq!(
            get_setting_value(&config, "typewriter_mode").expect("typewriter"),
            "true"
        );
        assert_eq!(
            get_setting_value(&config, "daily_word_goal").expect("word goal"),
            "750"
        );
    }

    #[test]
    fn set_setting_value_updates_paths_and_retention() {
        let mut config = AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: None,
            local_device_id: None,
            device_nickname: "This Mac".to_string(),
            typewriter_mode: false,
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };

        set_setting_value(&mut config, "sync_target_path", "/tmp/remote").expect("sync target");
        set_setting_value(&mut config, "backup_retention.weekly", "9").expect("weekly");
        set_setting_value(&mut config, "typewriter_mode", "true").expect("typewriter");
        set_setting_value(&mut config, "daily_word_goal", "500").expect("word goal");

        assert_eq!(config.sync_target_path, Some(PathBuf::from("/tmp/remote")));
        assert_eq!(config.backup_retention.weekly, 9);
        assert!(config.typewriter_mode);
        assert_eq!(config.daily_word_goal, Some(500));
    }

    #[test]
    fn set_setting_value_can_unset_optional_path() {
        let mut config = AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: Some(PathBuf::from("/tmp/remote")),
            local_device_id: None,
            device_nickname: "This Mac".to_string(),
            typewriter_mode: false,
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };

        set_setting_value(&mut config, "sync_target_path", "unset").expect("unset");
        assert!(config.sync_target_path.is_none());
    }
}
