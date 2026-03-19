use crate::secure_fs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use thiserror::Error;

const READABLE_SETTING_KEYS: &[&str] = &[
    "vault_path",
    "sync_target_path",
    "device_nickname",
    "typewriter_mode",
    "clock_12h",
    "show_seconds",
    "show_ruler",
    "show_footer_legend",
    "soundtrack_source",
    "opening_line_template",
    "daily_word_goal",
    "remember_passphrase_in_keychain",
    "backup_retention.daily",
    "backup_retention.weekly",
    "backup_retention.monthly",
    "local_device_id",
    "export_history",
    "search_presets",
];

const EDITABLE_SETTING_KEYS: &[&str] = &[
    "vault_path",
    "sync_target_path",
    "device_nickname",
    "typewriter_mode",
    "clock_12h",
    "show_seconds",
    "show_ruler",
    "show_footer_legend",
    "soundtrack_source",
    "opening_line_template",
    "daily_word_goal",
    "remember_passphrase_in_keychain",
    "backup_retention.daily",
    "backup_retention.weekly",
    "backup_retention.monthly",
];

const MAX_SEARCH_PRESETS: usize = 24;

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
    pub clock_12h: bool,
    #[serde(default)]
    pub show_seconds: bool,
    #[serde(default = "default_show_ruler")]
    pub show_ruler: bool,
    #[serde(default = "default_show_footer_legend")]
    pub show_footer_legend: bool,
    #[serde(default = "default_soundtrack_source")]
    pub soundtrack_source: String,
    #[serde(default = "default_opening_line_template")]
    pub opening_line_template: String,
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
    pub export_history: Vec<RecentExportInfo>,
    #[serde(default)]
    pub search_presets: Vec<SearchPresetConfig>,
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
pub struct RecentExportInfo {
    pub timestamp: String,
    pub date: String,
    pub format: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchPresetConfig {
    pub name: String,
    pub query: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
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
            Ok(Some(mut config)) => {
                normalize_search_presets(&mut config.search_presets);
                config
            }
            Ok(None) | Err(_) => Self {
                vault_path: default_vault_path(),
                sync_target_path: None,
                local_device_id: None,
                device_nickname: default_device_nickname(),
                typewriter_mode: false,
                clock_12h: false,
                show_seconds: false,
                show_ruler: default_show_ruler(),
                show_footer_legend: default_show_footer_legend(),
                soundtrack_source: default_soundtrack_source(),
                opening_line_template: default_opening_line_template(),
                daily_word_goal: None,
                remember_passphrase_in_keychain: false,
                first_run_coach_completed: false,
                last_sync: None,
                sync_history: Vec::new(),
                favorite_dates: Vec::new(),
                export_history: Vec::new(),
                search_presets: Vec::new(),
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

    pub fn search_preset(&self, name: &str) -> Option<&SearchPresetConfig> {
        let needle = name.trim();
        if needle.is_empty() {
            return None;
        }
        self.search_presets
            .iter()
            .find(|preset| preset.name.eq_ignore_ascii_case(needle))
    }

    pub fn upsert_search_preset(&mut self, preset: SearchPresetConfig) -> Result<(), String> {
        let preset = normalize_search_preset(preset)?;
        self.search_presets
            .retain(|existing| !existing.name.eq_ignore_ascii_case(&preset.name));
        self.search_presets.insert(0, preset);
        self.search_presets.truncate(MAX_SEARCH_PRESETS);
        Ok(())
    }

    pub fn remove_search_preset(&mut self, name: &str) -> bool {
        let needle = name.trim();
        if needle.is_empty() {
            return false;
        }
        let before = self.search_presets.len();
        self.search_presets
            .retain(|preset| !preset.name.eq_ignore_ascii_case(needle));
        self.search_presets.len() != before
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
        "clock_12h" => Ok(config.clock_12h.to_string()),
        "show_seconds" => Ok(config.show_seconds.to_string()),
        "show_ruler" => Ok(config.show_ruler.to_string()),
        "show_footer_legend" => Ok(config.show_footer_legend.to_string()),
        "soundtrack_source" => Ok(config.soundtrack_source.clone()),
        "opening_line_template" => Ok(config.opening_line_template.clone()),
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
        "export_history" => Ok(config.export_history.len().to_string()),
        "search_presets" => Ok(config.search_presets.len().to_string()),
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
        "clock_12h" => {
            config.clock_12h = parse_bool_value(value, key)?;
            Ok(config.clock_12h.to_string())
        }
        "show_seconds" => {
            config.show_seconds = parse_bool_value(value, key)?;
            Ok(config.show_seconds.to_string())
        }
        "show_ruler" => {
            config.show_ruler = parse_bool_value(value, key)?;
            Ok(config.show_ruler.to_string())
        }
        "show_footer_legend" => {
            config.show_footer_legend = parse_bool_value(value, key)?;
            Ok(config.show_footer_legend.to_string())
        }
        "soundtrack_source" => {
            config.soundtrack_source = value.trim().to_string();
            Ok(config.soundtrack_source.clone())
        }
        "opening_line_template" => {
            config.opening_line_template = value.trim().to_string();
            Ok(config.opening_line_template.clone())
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

fn normalize_search_presets(presets: &mut Vec<SearchPresetConfig>) {
    let source = std::mem::take(presets);
    let mut normalized = Vec::new();
    for preset in source {
        let Ok(preset) = normalize_search_preset(preset) else {
            continue;
        };
        if normalized
            .iter()
            .any(|existing: &SearchPresetConfig| existing.name.eq_ignore_ascii_case(&preset.name))
        {
            continue;
        }
        normalized.push(preset);
        if normalized.len() >= MAX_SEARCH_PRESETS {
            break;
        }
    }
    *presets = normalized;
}

fn normalize_search_preset(mut preset: SearchPresetConfig) -> Result<SearchPresetConfig, String> {
    let name = preset.name.trim();
    if name.is_empty() {
        return Err("search preset name cannot be empty".to_string());
    }
    let query = preset.query.trim();
    if query.is_empty() {
        return Err("search preset query cannot be empty".to_string());
    }
    preset.name = name.to_string();
    preset.query = query.to_string();
    preset.from = normalize_optional_text(preset.from.as_deref());
    preset.to = normalize_optional_text(preset.to.as_deref());
    Ok(preset)
}

fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
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

fn default_show_ruler() -> bool {
    true
}

fn default_show_footer_legend() -> bool {
    true
}

fn default_soundtrack_source() -> String {
    "https://www.midi-karaoke.info/21b56501.mid".to_string()
}

fn default_opening_line_template() -> String {
    "JOURNAL ENTRY {DATE}".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, BackupRetentionConfig, RecentExportInfo, SearchPresetConfig, default_vault_path,
        get_setting_value, set_setting_value,
    };
    use std::path::PathBuf;

    fn empty_test_config() -> AppConfig {
        AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: None,
            local_device_id: None,
            device_nickname: "This Mac".to_string(),
            typewriter_mode: false,
            clock_12h: false,
            show_seconds: false,
            show_ruler: true,
            show_footer_legend: true,
            soundtrack_source: String::new(),
            opening_line_template: "JOURNAL ENTRY {DATE}".to_string(),
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            export_history: Vec::new(),
            search_presets: Vec::new(),
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        }
    }

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
            clock_12h: true,
            show_seconds: true,
            show_ruler: false,
            show_footer_legend: false,
            soundtrack_source: "https://example.com/theme.mid".to_string(),
            opening_line_template: "SEAN'S JOURNAL ENTRY {DATE}".to_string(),
            daily_word_goal: Some(750),
            remember_passphrase_in_keychain: true,
            first_run_coach_completed: true,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: vec!["2026-03-17".to_string()],
            export_history: vec![RecentExportInfo {
                timestamp: "2026-03-17T01:02:03Z".to_string(),
                date: "2026-03-17".to_string(),
                format: "text".to_string(),
                path: "/tmp/entry.txt".to_string(),
            }],
            search_presets: Vec::new(),
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
            get_setting_value(&config, "clock_12h").expect("clock"),
            "true"
        );
        assert_eq!(
            get_setting_value(&config, "show_ruler").expect("ruler"),
            "false"
        );
        assert_eq!(
            get_setting_value(&config, "soundtrack_source").expect("soundtrack source"),
            "https://example.com/theme.mid"
        );
        assert_eq!(
            get_setting_value(&config, "opening_line_template").expect("opening line template"),
            "SEAN'S JOURNAL ENTRY {DATE}"
        );
        assert_eq!(
            get_setting_value(&config, "daily_word_goal").expect("word goal"),
            "750"
        );
        assert_eq!(
            get_setting_value(&config, "export_history").expect("export history"),
            "1"
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
            clock_12h: false,
            show_seconds: false,
            show_ruler: true,
            show_footer_legend: true,
            soundtrack_source: String::new(),
            opening_line_template: "JOURNAL ENTRY {DATE}".to_string(),
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            export_history: Vec::new(),
            search_presets: Vec::new(),
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };

        set_setting_value(&mut config, "sync_target_path", "/tmp/remote").expect("sync target");
        set_setting_value(&mut config, "backup_retention.weekly", "9").expect("weekly");
        set_setting_value(&mut config, "typewriter_mode", "true").expect("typewriter");
        set_setting_value(&mut config, "clock_12h", "true").expect("clock");
        set_setting_value(&mut config, "show_ruler", "false").expect("ruler");
        set_setting_value(
            &mut config,
            "soundtrack_source",
            "https://example.com/blue.mid",
        )
        .expect("soundtrack source");
        set_setting_value(
            &mut config,
            "opening_line_template",
            "SEAN'S JOURNAL ENTRY [TODAYSDATE]",
        )
        .expect("opening line template");
        set_setting_value(&mut config, "daily_word_goal", "500").expect("word goal");

        assert_eq!(config.sync_target_path, Some(PathBuf::from("/tmp/remote")));
        assert_eq!(config.backup_retention.weekly, 9);
        assert!(config.typewriter_mode);
        assert!(config.clock_12h);
        assert!(!config.show_ruler);
        assert_eq!(config.soundtrack_source, "https://example.com/blue.mid");
        assert_eq!(
            config.opening_line_template,
            "SEAN'S JOURNAL ENTRY [TODAYSDATE]"
        );
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
            clock_12h: false,
            show_seconds: false,
            show_ruler: true,
            show_footer_legend: true,
            soundtrack_source: String::new(),
            opening_line_template: "JOURNAL ENTRY {DATE}".to_string(),
            daily_word_goal: None,
            remember_passphrase_in_keychain: false,
            first_run_coach_completed: false,
            last_sync: None,
            sync_history: Vec::new(),
            favorite_dates: Vec::new(),
            export_history: Vec::new(),
            search_presets: Vec::new(),
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };

        set_setting_value(&mut config, "sync_target_path", "unset").expect("unset");
        assert!(config.sync_target_path.is_none());
    }

    #[test]
    fn upsert_search_preset_inserts_and_reorders_by_name() {
        let mut config = empty_test_config();

        config
            .upsert_search_preset(SearchPresetConfig {
                name: "Work".to_string(),
                query: "project alpha".to_string(),
                from: Some("2026-03-01".to_string()),
                to: Some("2026-03-19".to_string()),
            })
            .expect("insert preset");
        config
            .upsert_search_preset(SearchPresetConfig {
                name: "Morning".to_string(),
                query: "coffee".to_string(),
                from: None,
                to: None,
            })
            .expect("second preset");
        config
            .upsert_search_preset(SearchPresetConfig {
                name: "work".to_string(),
                query: "project beta".to_string(),
                from: None,
                to: None,
            })
            .expect("update preset");

        assert_eq!(config.search_presets.len(), 2);
        assert_eq!(config.search_presets[0].name, "work");
        assert_eq!(config.search_presets[0].query, "project beta");
        assert_eq!(config.search_presets[1].name, "Morning");
    }

    #[test]
    fn upsert_search_preset_rejects_blank_name_or_query() {
        let mut config = empty_test_config();

        let missing_name = config.upsert_search_preset(SearchPresetConfig {
            name: "   ".to_string(),
            query: "value".to_string(),
            from: None,
            to: None,
        });
        assert!(missing_name.is_err());

        let missing_query = config.upsert_search_preset(SearchPresetConfig {
            name: "Name".to_string(),
            query: "   ".to_string(),
            from: None,
            to: None,
        });
        assert!(missing_query.is_err());
    }

    #[test]
    fn remove_search_preset_matches_case_insensitive_name() {
        let mut config = empty_test_config();
        config.search_presets = vec![
            SearchPresetConfig {
                name: "Work".to_string(),
                query: "project".to_string(),
                from: None,
                to: None,
            },
            SearchPresetConfig {
                name: "Personal".to_string(),
                query: "family".to_string(),
                from: None,
                to: None,
            },
        ];

        assert!(config.remove_search_preset("work"));
        assert_eq!(config.search_presets.len(), 1);
        assert_eq!(config.search_presets[0].name, "Personal");
        assert!(!config.remove_search_preset("missing"));
    }
}
