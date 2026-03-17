use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf};
use thiserror::Error;

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
    pub backup_retention: BackupRetentionConfig,
    #[serde(default)]
    pub macros: Vec<MacroConfig>,
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
                backup_retention: BackupRetentionConfig::default(),
                macros: Vec::new(),
            },
        }
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = config_file_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp_path = path.with_extension("json.tmp");
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)?;

        let bytes = serde_json::to_vec_pretty(self)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(tmp_path, path)?;
        Ok(())
    }
}

pub fn default_vault_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("bluescreenjournal").join("vault")
}

fn config_file_path() -> Result<PathBuf, ConfigError> {
    let dir = dirs::config_dir().ok_or(ConfigError::MissingConfigDir)?;
    Ok(dir.join("bsj").join("config.json"))
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
