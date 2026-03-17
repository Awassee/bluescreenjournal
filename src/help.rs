use crate::{
    config::{self, AppConfig, MacroActionConfig, MacroCommandConfig},
    vault::VaultMetadata,
};
use serde_json::json;
use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

const SETUP_GUIDE_BODY: &str = include_str!("../docs/SETUP_GUIDE.md");
const PRODUCT_GUIDE_BODY: &str = include_str!("../docs/PRODUCT_GUIDE.md");
const DATASHEET_BODY: &str = include_str!("../docs/DATASHEET.md");
const SETTINGS_GUIDE_BODY: &str = include_str!("../docs/SETTINGS_GUIDE.md");
const DISTRIBUTION_GUIDE_BODY: &str = include_str!("../docs/DISTRIBUTION.md");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentSettings {
    pub passphrase_set: bool,
    pub sync_backend: Option<String>,
    pub s3_bucket_set: bool,
    pub s3_prefix_set: bool,
    pub aws_region_set: bool,
    pub webdav_url_set: bool,
    pub webdav_username_set: bool,
    pub webdav_password_set: bool,
}

impl EnvironmentSettings {
    pub fn capture() -> Self {
        Self {
            passphrase_set: std::env::var_os("BSJ_PASSPHRASE").is_some(),
            sync_backend: std::env::var("BSJ_SYNC_BACKEND").ok(),
            s3_bucket_set: std::env::var_os("BSJ_S3_BUCKET").is_some(),
            s3_prefix_set: std::env::var_os("BSJ_S3_PREFIX").is_some(),
            aws_region_set: std::env::var_os("AWS_REGION").is_some(),
            webdav_url_set: std::env::var_os("BSJ_WEBDAV_URL").is_some(),
            webdav_username_set: std::env::var_os("BSJ_WEBDAV_USERNAME").is_some(),
            webdav_password_set: std::env::var_os("BSJ_WEBDAV_PASSWORD").is_some(),
        }
    }
}

pub fn render_setup_guide(
    config_path: &Path,
    default_vault_path: &Path,
    log_path: &Path,
) -> String {
    format!(
        "BlueScreen Journal Setup Guide\n\
\n\
Actual paths on this Mac\n\
  Config file: {}\n\
  Default vault path: {}\n\
  Log file: {}\n\
\n\
{}\n",
        config_path.display(),
        default_vault_path.display(),
        log_path.display(),
        SETUP_GUIDE_BODY.trim_end()
    )
}

pub fn render_settings_guide(
    config_path: &Path,
    default_vault_path: &Path,
    log_path: &Path,
) -> String {
    format!(
        "BlueScreen Journal Settings Guide\n\
\n\
Reference paths\n\
  Config file: {}\n\
  Default vault path: {}\n\
  Log file: {}\n\
\n\
{}\n",
        config_path.display(),
        default_vault_path.display(),
        log_path.display(),
        SETTINGS_GUIDE_BODY.trim_end()
    )
}

pub fn render_product_guide() -> String {
    format!(
        "BlueScreen Journal Product Guide\n\n{}\n",
        PRODUCT_GUIDE_BODY.trim_end()
    )
}

pub fn render_datasheet() -> String {
    format!(
        "BlueScreen Journal Datasheet\n\n{}\n",
        DATASHEET_BODY.trim_end()
    )
}

pub fn render_distribution_guide() -> String {
    format!(
        "BlueScreen Journal Distribution Guide\n\n{}\n",
        DISTRIBUTION_GUIDE_BODY.trim_end()
    )
}

pub fn render_settings_report(
    config_path: &Path,
    config_exists: bool,
    config: &AppConfig,
    log_path: &Path,
    env: &EnvironmentSettings,
    vault_metadata: Option<&VaultMetadata>,
) -> String {
    let mut output = String::new();

    writeln!(&mut output, "BlueScreen Journal Settings").unwrap();
    writeln!(&mut output).unwrap();
    writeln!(&mut output, "Paths").unwrap();
    push_row(
        &mut output,
        "config_file",
        if config_exists {
            config_path.display().to_string()
        } else {
            format!("{} (missing, defaults in effect)", config_path.display())
        },
    );
    push_row(&mut output, "log_file", log_path.display().to_string());
    push_row(
        &mut output,
        "vault_path",
        config.vault_path.display().to_string(),
    );
    push_row(
        &mut output,
        "sync_target_path",
        option_path(&config.sync_target_path),
    );
    writeln!(&mut output).unwrap();

    writeln!(&mut output, "User-editable config").unwrap();
    push_row(
        &mut output,
        "device_nickname",
        config.device_nickname.clone(),
    );
    push_row(
        &mut output,
        "backup_retention",
        format!(
            "daily={} weekly={} monthly={}",
            config.backup_retention.daily,
            config.backup_retention.weekly,
            config.backup_retention.monthly
        ),
    );
    push_row(
        &mut output,
        "macros",
        if config.macros.is_empty() {
            "none configured".to_string()
        } else {
            format!("{} configured", config.macros.len())
        },
    );
    for macro_config in &config.macros {
        push_row(
            &mut output,
            &format!("  {}", macro_config.key),
            describe_macro_action(&macro_config.action),
        );
    }
    writeln!(&mut output).unwrap();

    writeln!(&mut output, "App-managed config").unwrap();
    push_row(
        &mut output,
        "local_device_id",
        option_string(&config.local_device_id),
    );
    writeln!(&mut output).unwrap();

    writeln!(&mut output, "Vault metadata").unwrap();
    if let Some(metadata) = vault_metadata {
        push_row(&mut output, "vault_json", "present".to_string());
        push_row(&mut output, "version", metadata.version.to_string());
        push_row(&mut output, "created_at", metadata.created_at.clone());
        push_row(&mut output, "device_id", metadata.device_id.clone());
        push_row(
            &mut output,
            "options.epoch_date",
            metadata.options.epoch_date.clone(),
        );
        push_row(&mut output, "kdf.algorithm", metadata.kdf.algorithm.clone());
        push_row(
            &mut output,
            "kdf.memory_kib",
            metadata.kdf.memory_kib.to_string(),
        );
        push_row(
            &mut output,
            "kdf.iterations",
            metadata.kdf.iterations.to_string(),
        );
        push_row(
            &mut output,
            "kdf.parallelism",
            metadata.kdf.parallelism.to_string(),
        );
        push_row(
            &mut output,
            "kdf.salt_hex",
            format!("present ({} hex chars)", metadata.kdf.salt_hex.len()),
        );
    } else {
        push_row(&mut output, "vault_json", "not initialized yet".to_string());
    }
    writeln!(&mut output).unwrap();

    writeln!(&mut output, "Environment").unwrap();
    push_row(
        &mut output,
        "BSJ_PASSPHRASE",
        set_status(env.passphrase_set),
    );
    push_row(
        &mut output,
        "BSJ_SYNC_BACKEND",
        env.sync_backend
            .clone()
            .unwrap_or_else(|| "unset".to_string()),
    );
    push_row(&mut output, "BSJ_S3_BUCKET", set_status(env.s3_bucket_set));
    push_row(&mut output, "BSJ_S3_PREFIX", set_status(env.s3_prefix_set));
    push_row(&mut output, "AWS_REGION", set_status(env.aws_region_set));
    push_row(
        &mut output,
        "BSJ_WEBDAV_URL",
        set_status(env.webdav_url_set),
    );
    push_row(
        &mut output,
        "BSJ_WEBDAV_USERNAME",
        set_status(env.webdav_username_set),
    );
    push_row(
        &mut output,
        "BSJ_WEBDAV_PASSWORD",
        set_status(env.webdav_password_set),
    );
    writeln!(&mut output).unwrap();

    writeln!(&mut output, "More help").unwrap();
    push_row(
        &mut output,
        "readable_keys",
        config::readable_setting_keys().join(", "),
    );
    push_row(
        &mut output,
        "editable_keys",
        config::editable_setting_keys().join(", "),
    );
    push_row(&mut output, "guide_setup", "bsj guide setup".to_string());
    push_row(
        &mut output,
        "guide_product",
        "bsj guide product".to_string(),
    );
    push_row(
        &mut output,
        "guide_datasheet",
        "bsj guide datasheet".to_string(),
    );
    push_row(
        &mut output,
        "guide_settings",
        "bsj guide settings".to_string(),
    );
    push_row(
        &mut output,
        "guide_distribution",
        "bsj guide distribution".to_string(),
    );
    push_row(
        &mut output,
        "machine_readable",
        "bsj settings --json".to_string(),
    );

    output
}

pub fn render_settings_json(
    config_path: &Path,
    config_exists: bool,
    config: &AppConfig,
    log_path: &Path,
    env: &EnvironmentSettings,
    vault_metadata: Option<&VaultMetadata>,
) -> serde_json::Value {
    json!({
        "paths": {
            "config_file": path_to_string(config_path),
            "config_file_exists": config_exists,
            "log_file": path_to_string(log_path),
            "vault_path": path_to_string(&config.vault_path),
            "sync_target_path": config.sync_target_path.as_deref().map(path_to_string),
        },
        "config": config,
        "vault_metadata": vault_metadata,
        "environment": {
            "BSJ_PASSPHRASE": { "set": env.passphrase_set },
            "BSJ_SYNC_BACKEND": env.sync_backend.clone(),
            "BSJ_S3_BUCKET": { "set": env.s3_bucket_set },
            "BSJ_S3_PREFIX": { "set": env.s3_prefix_set },
            "AWS_REGION": { "set": env.aws_region_set },
            "BSJ_WEBDAV_URL": { "set": env.webdav_url_set },
            "BSJ_WEBDAV_USERNAME": { "set": env.webdav_username_set },
            "BSJ_WEBDAV_PASSWORD": { "set": env.webdav_password_set },
        }
    })
}

fn push_row(output: &mut String, label: &str, value: String) {
    let _ = writeln!(output, "  {label:<20} {value}");
}

fn option_path(path: &Option<PathBuf>) -> String {
    path.as_deref()
        .map(path_to_string)
        .unwrap_or_else(|| "(unset)".to_string())
}

fn option_string(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "(unset)".to_string())
}

fn set_status(set: bool) -> String {
    if set {
        "set".to_string()
    } else {
        "unset".to_string()
    }
}

fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}

fn describe_macro_action(action: &MacroActionConfig) -> String {
    match action {
        MacroActionConfig::InsertTemplate { text } => {
            format!("insert_template ({} chars)", text.chars().count())
        }
        MacroActionConfig::Command { command } => {
            format!("command ({})", macro_command_name(command))
        }
    }
}

fn macro_command_name(command: &MacroCommandConfig) -> &'static str {
    match command {
        MacroCommandConfig::InsertDateHeader => "insert_date_header",
        MacroCommandConfig::InsertClosingLine => "insert_closing_line",
        MacroCommandConfig::JumpToday => "jump_today",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EnvironmentSettings, render_datasheet, render_product_guide, render_settings_guide,
        render_settings_report, render_setup_guide,
    };
    use crate::config::{AppConfig, BackupRetentionConfig, MacroActionConfig, MacroConfig};
    use crate::vault::{KdfParams, VaultMetadata, VaultOptions};
    use std::path::PathBuf;

    #[test]
    fn setup_guide_mentions_installer_and_commands() {
        let text = render_setup_guide(
            PathBuf::from("/tmp/config.json").as_path(),
            PathBuf::from("/tmp/vault").as_path(),
            PathBuf::from("/tmp/bsj.log").as_path(),
        );
        assert!(text.contains("./install.sh"));
        assert!(text.contains("bsj guide settings"));
        assert!(text.contains("/tmp/config.json"));
    }

    #[test]
    fn settings_guide_mentions_all_setting_surfaces() {
        let text = render_settings_guide(
            PathBuf::from("/tmp/config.json").as_path(),
            PathBuf::from("/tmp/vault").as_path(),
            PathBuf::from("/tmp/bsj.log").as_path(),
        );
        assert!(text.contains("vault_path"));
        assert!(text.contains("BSJ_SYNC_BACKEND"));
        assert!(text.contains("epochDate"));
    }

    #[test]
    fn product_guide_mentions_core_workflow_and_value() {
        let text = render_product_guide();
        assert!(text.contains("Start writing"));
        assert!(text.contains("Menu-driven TUI"));
        assert!(text.contains("append-only"));
    }

    #[test]
    fn datasheet_mentions_install_surface_and_guides() {
        let text = render_datasheet();
        assert!(text.contains("curl -fsSL"));
        assert!(text.contains("bsj guide product"));
        assert!(text.contains("BlueScreen Journal (`bsj`)"));
    }

    #[test]
    fn settings_report_includes_redacted_environment_status() {
        let config = AppConfig {
            vault_path: PathBuf::from("/tmp/vault"),
            sync_target_path: Some(PathBuf::from("/tmp/remote")),
            local_device_id: Some("abc123".to_string()),
            device_nickname: "QA Mac".to_string(),
            backup_retention: BackupRetentionConfig {
                daily: 7,
                weekly: 4,
                monthly: 6,
            },
            macros: vec![MacroConfig {
                key: "ctrl-j".to_string(),
                action: MacroActionConfig::InsertTemplate {
                    text: "TODAY\n".to_string(),
                },
            }],
        };
        let metadata = VaultMetadata {
            version: 1,
            created_at: "2026-03-16T12:00:00Z".to_string(),
            device_id: "device01".to_string(),
            kdf: KdfParams {
                algorithm: "argon2id".to_string(),
                memory_kib: 65_536,
                iterations: 3,
                parallelism: 1,
                salt_hex: "abcd".repeat(8),
            },
            options: VaultOptions {
                epoch_date: "2026-03-16".to_string(),
            },
        };
        let env = EnvironmentSettings {
            passphrase_set: true,
            sync_backend: Some("folder".to_string()),
            s3_bucket_set: false,
            s3_prefix_set: false,
            aws_region_set: false,
            webdav_url_set: true,
            webdav_username_set: true,
            webdav_password_set: true,
        };

        let report = render_settings_report(
            PathBuf::from("/tmp/config.json").as_path(),
            true,
            &config,
            PathBuf::from("/tmp/bsj.log").as_path(),
            &env,
            Some(&metadata),
        );

        assert!(report.contains("BSJ_PASSPHRASE"));
        assert!(report.contains("set"));
        assert!(report.contains("insert_template"));
        assert!(report.contains("present (32 hex chars)"));
        assert!(!report.contains("TODAY\n"));
    }
}
