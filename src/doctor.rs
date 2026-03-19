use crate::{
    config::AppConfig,
    help::EnvironmentSettings,
    vault::{IntegrityReport, VaultMetadata},
};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: String,
    pub status: DoctorStatus,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

pub struct DoctorInputs<'a> {
    pub config_path: &'a Path,
    pub config_exists: bool,
    pub config_error: Option<&'a str>,
    pub config: &'a AppConfig,
    pub log_path: &'a Path,
    pub env: &'a EnvironmentSettings,
    pub vault_exists: bool,
    pub vault_metadata: Option<&'a VaultMetadata>,
    pub vault_metadata_error: Option<&'a str>,
    pub integrity_report: Option<&'a IntegrityReport>,
    pub unlock_error: Option<&'a str>,
    pub entry_count: Option<usize>,
    pub backup_count: Option<usize>,
    pub conflict_count: Option<usize>,
}

pub fn build_report(input: DoctorInputs<'_>) -> DoctorReport {
    let mut checks = Vec::new();

    if let Some(error) = input.config_error {
        checks.push(check("config", DoctorStatus::Fail, error.to_string()));
    } else if input.config_exists {
        checks.push(check(
            "config",
            DoctorStatus::Ok,
            format!("config loaded from {}", input.config_path.display()),
        ));
    } else {
        checks.push(check(
            "config",
            DoctorStatus::Warn,
            format!(
                "config file missing; defaults are active at {}",
                input.config_path.display()
            ),
        ));
    }

    let log_dir = input
        .log_path
        .parent()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "(unknown)".to_string());
    checks.push(check(
        "logging",
        DoctorStatus::Ok,
        format!("log file is {}", input.log_path.display()),
    ));
    checks.push(check(
        "log_directory",
        if Path::new(&log_dir).exists() {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Warn
        },
        format!("log directory {}", log_dir),
    ));

    if input.vault_exists {
        checks.push(check(
            "vault_path",
            DoctorStatus::Ok,
            format!("vault found at {}", input.config.vault_path.display()),
        ));
    } else {
        checks.push(check(
            "vault_path",
            DoctorStatus::Warn,
            format!(
                "vault not initialized at {}",
                input.config.vault_path.display()
            ),
        ));
    }

    if let Some(error) = input.vault_metadata_error {
        checks.push(check(
            "vault_metadata",
            DoctorStatus::Fail,
            error.to_string(),
        ));
    } else if let Some(metadata) = input.vault_metadata {
        let epoch_detail = match metadata.epoch_date() {
            Ok(epoch) => format!(
                "vault metadata readable; epoch={}, device_id={}",
                epoch.format("%Y-%m-%d"),
                metadata.device_id
            ),
            Err(error) => error.to_string(),
        };
        let status = if metadata.epoch_date().is_ok() {
            DoctorStatus::Ok
        } else {
            DoctorStatus::Fail
        };
        checks.push(check("vault_metadata", status, epoch_detail));
    }

    let sync_status = match input
        .env
        .sync_backend
        .as_deref()
        .map(|value| value.to_ascii_lowercase())
    {
        Some(value) if value == "folder" => {
            if let Some(path) = &input.config.sync_target_path {
                if path.exists() {
                    check(
                        "sync",
                        DoctorStatus::Ok,
                        format!("folder sync target {}", path.display()),
                    )
                } else {
                    check(
                        "sync",
                        DoctorStatus::Warn,
                        format!("folder sync target missing: {}", path.display()),
                    )
                }
            } else {
                check(
                    "sync",
                    DoctorStatus::Warn,
                    "BSJ_SYNC_BACKEND=folder is set but sync_target_path is not configured"
                        .to_string(),
                )
            }
        }
        Some(value) if value == "s3" => {
            if input.env.s3_bucket_set {
                check(
                    "sync",
                    DoctorStatus::Ok,
                    "S3 sync environment looks configured".to_string(),
                )
            } else {
                check(
                    "sync",
                    DoctorStatus::Fail,
                    "BSJ_SYNC_BACKEND=s3 is set but BSJ_S3_BUCKET is missing".to_string(),
                )
            }
        }
        Some(value) if value == "webdav" => {
            if !input.env.webdav_url_set {
                check(
                    "sync",
                    DoctorStatus::Fail,
                    "BSJ_SYNC_BACKEND=webdav is set but BSJ_WEBDAV_URL is missing".to_string(),
                )
            } else if input.env.webdav_username_set && !input.env.webdav_password_set {
                check(
                    "sync",
                    DoctorStatus::Fail,
                    "BSJ_WEBDAV_USERNAME is set but BSJ_WEBDAV_PASSWORD is missing".to_string(),
                )
            } else {
                check(
                    "sync",
                    DoctorStatus::Ok,
                    "WebDAV sync environment looks configured".to_string(),
                )
            }
        }
        Some(value) => check(
            "sync",
            DoctorStatus::Warn,
            format!("unknown BSJ_SYNC_BACKEND value '{value}'"),
        ),
        None => {
            if let Some(path) = &input.config.sync_target_path {
                let status = if path.exists() {
                    DoctorStatus::Ok
                } else {
                    DoctorStatus::Warn
                };
                check(
                    "sync",
                    status,
                    format!("folder sync target remembered at {}", path.display()),
                )
            } else {
                check(
                    "sync",
                    DoctorStatus::Warn,
                    "no default sync target configured".to_string(),
                )
            }
        }
    };
    checks.push(sync_status);

    match (input.integrity_report, input.unlock_error) {
        (Some(report), _) if report.ok => {
            checks.push(check(
                "integrity",
                DoctorStatus::Ok,
                "hashchain verification passed".to_string(),
            ));
        }
        (Some(report), _) => {
            checks.push(check(
                "integrity",
                DoctorStatus::Fail,
                format!("{} issue(s) detected", report.issues.len()),
            ));
        }
        (None, Some(error)) => {
            checks.push(check("integrity", DoctorStatus::Fail, error.to_string()));
        }
        (None, None) if input.vault_exists => {
            checks.push(check(
                "integrity",
                DoctorStatus::Warn,
                "integrity not checked; rerun with --unlock".to_string(),
            ));
        }
        (None, None) => {}
    }

    if let Some(entry_count) = input.entry_count {
        let conflicts = input.conflict_count.unwrap_or(0);
        let backups = input.backup_count.unwrap_or(0);
        checks.push(check(
            "vault_contents",
            if conflicts == 0 {
                DoctorStatus::Ok
            } else {
                DoctorStatus::Warn
            },
            format!("{entry_count} entries, {backups} backups, {conflicts} conflicted dates"),
        ));
    }

    let ok = !checks
        .iter()
        .any(|check| matches!(check.status, DoctorStatus::Fail));

    DoctorReport { ok, checks }
}

pub fn render_text(report: &DoctorReport) -> String {
    let mut output = String::new();
    output.push_str("BlueScreen Journal Doctor\n\n");
    for check in &report.checks {
        let status = match check.status {
            DoctorStatus::Ok => "OK",
            DoctorStatus::Warn => "WARN",
            DoctorStatus::Fail => "FAIL",
        };
        output.push_str(&format!(
            "{status:<5} {:<18} {}\n",
            check.name, check.detail
        ));
    }
    output.push_str(&format!(
        "\nSummary: {}\n",
        if report.ok { "OK" } else { "ACTION REQUIRED" }
    ));
    output
}

fn check(name: &str, status: DoctorStatus, detail: String) -> DoctorCheck {
    DoctorCheck {
        name: name.to_string(),
        status,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::{DoctorInputs, DoctorStatus, build_report, render_text};
    use crate::{
        config::{AppConfig, BackupRetentionConfig},
        help::EnvironmentSettings,
        vault::{KdfParams, VaultMetadata, VaultOptions},
    };
    use std::path::PathBuf;

    #[test]
    fn doctor_reports_missing_config_and_vault_as_warnings() {
        let config = AppConfig {
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
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };
        let env = EnvironmentSettings::default();
        let report = build_report(DoctorInputs {
            config_path: PathBuf::from("/tmp/config.json").as_path(),
            config_exists: false,
            config_error: None,
            config: &config,
            log_path: PathBuf::from("/tmp/bsj.log").as_path(),
            env: &env,
            vault_exists: false,
            vault_metadata: None,
            vault_metadata_error: None,
            integrity_report: None,
            unlock_error: None,
            entry_count: None,
            backup_count: None,
            conflict_count: None,
        });

        assert!(report.ok);
        assert!(render_text(&report).contains("WARN"));
    }

    #[test]
    fn doctor_reports_invalid_sync_environment_as_failure() {
        let config = AppConfig {
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
            backup_retention: BackupRetentionConfig::default(),
            macros: Vec::new(),
        };
        let env = EnvironmentSettings {
            sync_backend: Some("s3".to_string()),
            ..EnvironmentSettings::default()
        };
        let metadata = VaultMetadata {
            version: 1,
            created_at: "2026-03-17T12:00:00Z".to_string(),
            device_id: "device".to_string(),
            kdf: KdfParams {
                algorithm: "argon2id".to_string(),
                memory_kib: 65536,
                iterations: 3,
                parallelism: 1,
                salt_hex: "abcd".repeat(8),
            },
            options: VaultOptions {
                epoch_date: "2026-03-17".to_string(),
            },
        };

        let report = build_report(DoctorInputs {
            config_path: PathBuf::from("/tmp/config.json").as_path(),
            config_exists: true,
            config_error: None,
            config: &config,
            log_path: PathBuf::from("/tmp/bsj.log").as_path(),
            env: &env,
            vault_exists: true,
            vault_metadata: Some(&metadata),
            vault_metadata_error: None,
            integrity_report: None,
            unlock_error: None,
            entry_count: None,
            backup_count: None,
            conflict_count: None,
        });

        assert!(!report.ok);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "sync" && check.status == DoctorStatus::Fail)
        );
    }
}
