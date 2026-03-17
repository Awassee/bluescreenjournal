mod config;
mod doctor;
mod help;
mod logging;
mod search;
mod sync;
mod tui;
mod vault;

use chrono::NaiveDate;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use secrecy::SecretString;
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
#[command(
    name = "bsj",
    about = "BlueScreen Journal",
    long_about = "BlueScreen Journal is an encrypted, local-first macOS terminal journal with a nostalgic blue-screen full-screen editor, a menu-driven TUI, append-only revisions, encrypted drafts, encrypted backups, and encrypted sync targets.",
    after_help = "Examples:\n  bsj\n  bsj open 2026-03-16\n  bsj search \"quiet morning\" --from 2026-03-01 --to 2026-03-31\n  bsj export 2026-03-16 --format markdown --output ~/Desktop/entry.md\n  bsj sync --backend folder --remote ~/Documents/BlueScreenJournal-Sync\n  bsj backup\n  bsj backup list\n  bsj backup prune --apply\n  bsj settings init\n  bsj settings get vault_path\n  bsj settings set sync_target_path ~/Documents/BlueScreenJournal-Sync\n  bsj doctor --unlock\n  bsj completions zsh\n\nGuides:\n  bsj guide product\n  bsj guide datasheet\n  bsj guide setup\n  bsj guide settings\n  bsj guide distribution\n\nPackaging:\n  ./install.sh --prebuilt\n  ./scripts/package-release.sh\n\nDebug logging:\n  Use --debug to enable verbose file logging at ~/Library/Logs/bsj/bsj.log"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Enable verbose debug logging to ~/Library/Logs/bsj/bsj.log"
    )]
    debug: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open a specific journal date in the TUI
    Open { date: String },
    /// Export a date as plain text or markdown
    Export {
        date: String,
        #[arg(long, value_enum, default_value_t = ExportFormatArg::Text)]
        format: ExportFormatArg,
        #[arg(long, help = "Write the export to a file instead of stdout")]
        output: Option<String>,
    },
    /// Search encrypted entries without writing a plaintext index
    Search {
        query: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    /// Sync encrypted revisions with a folder, S3, or WebDAV target
    Sync {
        #[arg(long, value_enum)]
        backend: Option<SyncBackendArg>,
        #[arg(long)]
        remote: Option<String>,
    },
    /// Create, list, or prune encrypted backups under <vault>/backups
    Backup {
        #[command(subcommand)]
        command: Option<BackupCommand>,
    },
    /// Restore an encrypted backup snapshot into a target directory
    Restore {
        backup: String,
        #[arg(long)]
        into: String,
    },
    /// Verify revision hashchains
    Verify,
    /// Show effective settings or manage config values
    Settings {
        #[arg(long, help = "Print the current settings report in JSON")]
        json: bool,
        #[command(subcommand)]
        command: Option<SettingsCommand>,
    },
    /// Print the setup, product, datasheet, settings, or distribution guide
    Guide {
        #[arg(value_enum, default_value_t = GuideTopicArg::Setup)]
        topic: GuideTopicArg,
    },
    /// Diagnose install, config, sync, and vault health
    Doctor {
        #[arg(long, help = "Unlock the vault and run encrypted integrity checks")]
        unlock: bool,
        #[arg(long, help = "Print the doctor report in JSON")]
        json: bool,
    },
    /// Generate shell completion scripts to stdout
    Completions {
        #[arg(value_enum)]
        shell: CompletionShellArg,
    },
}

#[derive(Debug, Subcommand)]
enum BackupCommand {
    /// List encrypted backups under <vault>/backups
    List,
    /// Show or apply retention-based backup pruning
    Prune {
        #[arg(long, help = "Delete the backups instead of showing a dry run")]
        apply: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    /// Create a default config file if one does not exist
    Init {
        #[arg(long, help = "Overwrite an existing or broken config file")]
        force: bool,
    },
    /// Read one setting value
    Get { key: String },
    /// Update one editable setting value
    Set { key: String, value: String },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SyncBackendArg {
    Folder,
    S3,
    Webdav,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum GuideTopicArg {
    Setup,
    Product,
    Datasheet,
    Settings,
    Distribution,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExportFormatArg {
    Text,
    Markdown,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CompletionShellArg {
    Bash,
    Fish,
    Zsh,
}

struct LoadedConfigState {
    path: PathBuf,
    config: config::AppConfig,
    exists: bool,
    error: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = logging::init(cli.debug) {
        eprintln!("failed to initialize logging: {error}");
        std::process::exit(1);
    }
    log::info!("bsj starting");

    match cli.command {
        Some(Command::Open { date }) => {
            let initial_date = match parse_date_arg("date", &date) {
                Ok(date) => Some(date),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            log::info!("launching TUI for requested date");
            if let Err(error) = tui::run(initial_date) {
                log::error!("failed to launch TUI: {error}");
                eprintln!("failed to launch TUI: {error}");
                std::process::exit(1);
            }
        }
        Some(Command::Export {
            date,
            format,
            output,
        }) => {
            let date = match parse_date_arg("date", &date) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_export(date, format, output.as_deref()) {
                log::error!("export failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Search { query, from, to }) => {
            let from = match parse_optional_date_arg("from", from.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };
            let to = match parse_optional_date_arg("to", to.as_deref()) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_search(&query, from, to) {
                log::error!("search failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Sync { backend, remote }) => {
            if let Err(error) = run_cli_sync(backend, remote.as_deref()) {
                log::error!("sync failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Backup { command }) => {
            if let Err(error) = run_cli_backup(command) {
                log::error!("backup failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Restore { backup, into }) => {
            if let Err(error) = run_cli_restore(&backup, &into) {
                log::error!("restore failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Verify) => {
            if let Err(error) = run_cli_verify() {
                log::error!("verify failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Settings { json, command }) => {
            if let Err(error) = run_cli_settings(json, command) {
                log::error!("settings failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Guide { topic }) => {
            if let Err(error) = run_cli_guide(topic) {
                log::error!("guide failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Doctor { unlock, json }) => match run_cli_doctor(unlock, json) {
            Ok(true) => {}
            Ok(false) => std::process::exit(1),
            Err(error) => {
                log::error!("doctor failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        },
        Some(Command::Completions { shell }) => {
            if let Err(error) = run_cli_completions(shell) {
                log::error!("completion generation failed: {error}");
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        None => {
            log::info!("launching TUI");
            if let Err(error) = tui::run(None) {
                log::error!("failed to launch TUI: {error}");
                eprintln!("failed to launch TUI: {error}");
                std::process::exit(1);
            }
        }
    }
}

fn parse_optional_date_arg(label: &str, value: Option<&str>) -> Result<Option<NaiveDate>, String> {
    value.map(|value| parse_date_arg(label, value)).transpose()
}

fn parse_date_arg(label: &str, value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("invalid {label} '{value}'; expected YYYY-MM-DD"))
}

fn run_cli_search(
    query: &str,
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<(), String> {
    log::info!("running CLI search");
    if let (Some(from), Some(to)) = (from, to)
        && from > to
    {
        return Err("--from cannot be after --to".to_string());
    }

    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let documents = vault
        .load_search_documents()
        .map_err(|error| format!("failed to read entries: {error}"))?;
    let index = search::SearchIndex::build(documents);
    let results = index.search(&search::SearchQuery {
        text: query.to_string(),
        from,
        to,
    });

    if results.is_empty() {
        println!("No matches.");
        return Ok(());
    }

    for result in results {
        println!(
            "{}:{}:{}  {}",
            result.date.format("%Y-%m-%d"),
            result.row + 1,
            result.start_col + 1,
            search::format_cli_snippet(&result.snippet)
        );
    }

    Ok(())
}

fn run_cli_export(
    date: NaiveDate,
    format: ExportFormatArg,
    output: Option<&str>,
) -> Result<(), String> {
    log::info!("running CLI export");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let Some(entry) = vault
        .export_entry(date)
        .map_err(|error| format!("export failed: {error}"))?
    else {
        return Err(format!("no saved entry for {}", date.format("%Y-%m-%d")));
    };

    let rendered = match format {
        ExportFormatArg::Text => {
            vault::format_export_text(&entry.body, entry.closing_thought.as_deref())
        }
        ExportFormatArg::Markdown => render_markdown_export(&entry),
    };

    if let Some(output) = output {
        let path = config::expand_path_like(output);
        write_plaintext_output(&path, &rendered)?;
        println!("Wrote: {}", path.display());
    } else {
        println!("{rendered}");
    }

    Ok(())
}

fn run_cli_sync(
    backend_arg: Option<SyncBackendArg>,
    remote_arg: Option<&str>,
) -> Result<(), String> {
    log::info!("running CLI sync");
    let mut config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let backend_kind = resolve_sync_backend_kind(backend_arg, remote_arg)?;

    let report = match backend_kind {
        SyncBackendArg::Folder => {
            let remote_root = resolve_folder_sync_target_path(&mut config, remote_arg)?;
            vault
                .sync_folder(&remote_root)
                .map_err(|error| format!("sync failed: {error}"))?
        }
        SyncBackendArg::S3 => {
            let mut backend = sync::S3Backend::from_remote(remote_arg)?;
            vault
                .sync_with_backend(&mut backend)
                .map_err(|error| format!("sync failed: {error}"))?
        }
        SyncBackendArg::Webdav => {
            let mut backend = sync::WebDavBackend::from_remote(remote_arg)?;
            vault
                .sync_with_backend(&mut backend)
                .map_err(|error| format!("sync failed: {error}"))?
        }
    };

    println!("Pulled: {}", report.pulled);
    println!("Pushed: {}", report.pushed);
    if report.conflicts.is_empty() {
        println!("Conflicts: none");
    } else {
        println!("Conflicts:");
        for date in report.conflicts {
            println!("  {}", date.format("%Y-%m-%d"));
        }
    }

    Ok(())
}

fn run_cli_verify() -> Result<(), String> {
    log::info!("running CLI verify");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let report = vault
        .verify_integrity()
        .map_err(|error| format!("verify failed: {error}"))?;

    if report.ok {
        println!("OK");
    } else {
        println!("BROKEN");
        for issue in report.issues {
            match issue.date {
                Some(date) => println!("{}  {}", date.format("%Y-%m-%d"), issue.message),
                None => println!("{}", issue.message),
            }
        }
    }
    Ok(())
}

fn run_cli_backup(command: Option<BackupCommand>) -> Result<(), String> {
    log::info!("running CLI backup");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;

    match command {
        None => {
            let summary = vault
                .create_backup(&config.backup_retention)
                .map_err(|error| format!("backup failed: {error}"))?;
            println!("Backup: {}", summary.path.display());
            println!("Pruned: {}", summary.pruned);
        }
        Some(BackupCommand::List) => {
            let backups = vault
                .list_backups()
                .map_err(|error| format!("backup list failed: {error}"))?;
            if backups.is_empty() {
                println!("No backups.");
            } else {
                for backup in backups {
                    println!(
                        "{}  {:>10} bytes  {}",
                        backup.created_at.format("%Y-%m-%d %H:%M:%SZ"),
                        backup.size_bytes,
                        backup.path.display()
                    );
                }
            }
        }
        Some(BackupCommand::Prune { apply }) => {
            let pruned = if apply {
                vault
                    .prune_backups_now(&config.backup_retention)
                    .map_err(|error| format!("backup prune failed: {error}"))?
            } else {
                vault
                    .preview_backup_prune(&config.backup_retention)
                    .map_err(|error| format!("backup prune preview failed: {error}"))?
            };
            if pruned.is_empty() {
                println!("No backups to prune.");
            } else {
                println!(
                    "{} {} backup(s):",
                    if apply { "Pruned" } else { "Would prune" },
                    pruned.len()
                );
                for backup in pruned {
                    println!(
                        "  {}  {}",
                        backup.created_at.format("%Y-%m-%d %H:%M:%SZ"),
                        backup.path.display()
                    );
                }
                if !apply {
                    println!("Dry run only. Re-run with --apply to delete these backups.");
                }
            }
        }
    }

    Ok(())
}

fn run_cli_restore(backup: &str, into: &str) -> Result<(), String> {
    log::info!("running CLI restore");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let backup_path = config::expand_path_like(backup);
    let restore_root = config::expand_path_like(into);
    vault
        .restore_backup_into(&backup_path, &restore_root)
        .map_err(|error| format!("restore failed: {error}"))?;
    println!("Restored: {}", restore_root.display());
    Ok(())
}

fn run_cli_settings(json_output: bool, command: Option<SettingsCommand>) -> Result<(), String> {
    log::info!("running CLI settings");
    let state = load_config_state();

    match command {
        None => render_settings_report(&state, json_output),
        Some(SettingsCommand::Init { force }) => {
            if json_output {
                return Err("--json cannot be combined with settings init".to_string());
            }
            run_cli_settings_init(state, force)
        }
        Some(SettingsCommand::Get { key }) => {
            if json_output {
                return Err("--json cannot be combined with settings get".to_string());
            }
            if let Some(error) = state.error.as_deref() {
                return Err(format!(
                    "config is invalid: {error}. Fix it or run `bsj settings init --force`."
                ));
            }
            println!("{}", config::get_setting_value(&state.config, &key)?);
            Ok(())
        }
        Some(SettingsCommand::Set { key, value }) => {
            if json_output {
                return Err("--json cannot be combined with settings set".to_string());
            }
            if state.exists && state.error.is_some() {
                return Err(
                    "config is invalid. Fix it or run `bsj settings init --force` before using settings set."
                        .to_string(),
                );
            }
            let mut config = state.config.clone();
            let updated = config::set_setting_value(&mut config, &key, &value)?;
            config
                .save()
                .map_err(|error| format!("failed to save config: {error}"))?;
            println!("{key}={updated}");
            Ok(())
        }
    }
}

fn render_settings_report(state: &LoadedConfigState, json_output: bool) -> Result<(), String> {
    let log_path = logging::log_file_path();
    let env = help::EnvironmentSettings::capture();
    let (vault_metadata, vault_metadata_error) = load_vault_metadata_state(&state.config);

    if let Some(error) = state.error.as_deref()
        && json_output
    {
        let document = serde_json::json!({
            "config_error": error,
            "paths": {
                "config_file": state.path.display().to_string(),
                "config_file_exists": state.exists,
                "log_file": log_path.display().to_string(),
            }
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&document)
                .map_err(|error| format!("failed to serialize settings JSON: {error}"))?
        );
        return Ok(());
    }

    if let Some(error) = state.error.as_deref() {
        return Err(format!(
            "config is invalid: {error}. Fix it or run `bsj settings init --force`."
        ));
    }

    if json_output {
        let document = help::render_settings_json(
            &state.path,
            state.exists,
            &state.config,
            &log_path,
            &env,
            vault_metadata.as_ref(),
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&document)
                .map_err(|error| format!("failed to serialize settings JSON: {error}"))?
        );
    } else {
        let mut report = help::render_settings_report(
            &state.path,
            state.exists,
            &state.config,
            &log_path,
            &env,
            vault_metadata.as_ref(),
        );
        if let Some(error) = vault_metadata_error {
            report.push_str("\nWarnings\n");
            report.push_str(&format!("  vault_metadata_error   {error}\n"));
        }
        println!("{report}");
    }
    Ok(())
}

fn run_cli_settings_init(state: LoadedConfigState, force: bool) -> Result<(), String> {
    if state.exists && state.error.is_none() && !force {
        return Err(format!(
            "config already exists at {}. Re-run with --force to overwrite.",
            state.path.display()
        ));
    }
    if state.exists && state.error.is_some() && !force {
        return Err(format!(
            "config at {} is invalid. Re-run with --force to replace it.",
            state.path.display()
        ));
    }

    let config = if force && state.error.is_some() {
        config::AppConfig::load_or_default()
    } else {
        state.config
    };
    config
        .save()
        .map_err(|error| format!("failed to write config: {error}"))?;
    println!("Initialized: {}", state.path.display());
    Ok(())
}

fn run_cli_guide(topic: GuideTopicArg) -> Result<(), String> {
    log::info!("printing guide");
    let config_path = config::config_file_path()
        .unwrap_or_else(|_| fallback_config_path("bsj").join("config.json"));
    let default_vault_path = config::default_vault_path();
    let log_path = logging::log_file_path();

    let output = match topic {
        GuideTopicArg::Setup => {
            help::render_setup_guide(&config_path, &default_vault_path, &log_path)
        }
        GuideTopicArg::Product => help::render_product_guide(),
        GuideTopicArg::Datasheet => help::render_datasheet(),
        GuideTopicArg::Settings => {
            help::render_settings_guide(&config_path, &default_vault_path, &log_path)
        }
        GuideTopicArg::Distribution => help::render_distribution_guide(),
    };

    println!("{output}");
    Ok(())
}

fn run_cli_doctor(unlock: bool, json_output: bool) -> Result<bool, String> {
    log::info!("running CLI doctor");
    let state = load_config_state();
    let log_path = logging::log_file_path();
    let env = help::EnvironmentSettings::capture();
    let (vault_metadata, vault_metadata_error) = load_vault_metadata_state(&state.config);
    let vault_exists = vault::vault_exists(&state.config.vault_path);

    let mut integrity_report = None;
    let mut unlock_error = None;
    let mut entry_count = None;
    let mut backup_count = None;
    let mut conflict_count = None;

    if unlock && vault_exists && state.error.is_none() && vault_metadata_error.is_none() {
        match unlock_cli_vault(&state.config) {
            Ok(vault) => {
                match vault.verify_integrity() {
                    Ok(report) => integrity_report = Some(report),
                    Err(error) => unlock_error = Some(format!("integrity check failed: {error}")),
                }

                if unlock_error.is_none() {
                    entry_count = Some(
                        vault
                            .list_entry_dates()
                            .map_err(|error| format!("failed to count entries: {error}"))?
                            .len(),
                    );
                    backup_count = Some(
                        vault
                            .list_backups()
                            .map_err(|error| format!("failed to count backups: {error}"))?
                            .len(),
                    );
                    conflict_count = Some(
                        vault
                            .list_conflicted_dates()
                            .map_err(|error| format!("failed to count conflicts: {error}"))?
                            .len(),
                    );
                }
            }
            Err(error) => unlock_error = Some(error),
        }
    }

    let report = doctor::build_report(doctor::DoctorInputs {
        config_path: &state.path,
        config_exists: state.exists,
        config_error: state.error.as_deref(),
        config: &state.config,
        log_path: &log_path,
        env: &env,
        vault_exists,
        vault_metadata: vault_metadata.as_ref(),
        vault_metadata_error: vault_metadata_error.as_deref(),
        integrity_report: integrity_report.as_ref(),
        unlock_error: unlock_error.as_deref(),
        entry_count,
        backup_count,
        conflict_count,
    });

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|error| format!("failed to serialize doctor JSON: {error}"))?
        );
    } else {
        println!("{}", doctor::render_text(&report));
    }

    Ok(report.ok)
}

fn run_cli_completions(shell: CompletionShellArg) -> Result<(), String> {
    log::info!("generating completions");
    let mut command = Cli::command();
    let shell = match shell {
        CompletionShellArg::Bash => Shell::Bash,
        CompletionShellArg::Fish => Shell::Fish,
        CompletionShellArg::Zsh => Shell::Zsh,
    };
    let mut stdout = io::stdout();
    generate(shell, &mut command, "bsj", &mut stdout);
    Ok(())
}

fn unlock_cli_vault(config: &config::AppConfig) -> Result<vault::UnlockedVault, String> {
    if !vault::vault_exists(&config.vault_path) {
        return Err(format!(
            "vault not found at {}",
            config.vault_path.display()
        ));
    }

    let secret = read_cli_secret()?;
    vault::unlock_vault(&config.vault_path, &secret)
        .map_err(|error| format!("unlock failed: {error}"))
}

fn read_cli_secret() -> Result<SecretString, String> {
    let passphrase = match env::var("BSJ_PASSPHRASE") {
        Ok(passphrase) => passphrase,
        Err(_) => rpassword::prompt_password("Vault passphrase: ")
            .map_err(|error| format!("failed to read passphrase: {error}"))?,
    };
    Ok(SecretString::new(passphrase.into_boxed_str()))
}

fn resolve_sync_backend_kind(
    backend_arg: Option<SyncBackendArg>,
    remote_arg: Option<&str>,
) -> Result<SyncBackendArg, String> {
    if let Some(backend_arg) = backend_arg {
        return Ok(backend_arg);
    }

    if let Ok(value) = env::var("BSJ_SYNC_BACKEND") {
        return match value.to_ascii_lowercase().as_str() {
            "folder" => Ok(SyncBackendArg::Folder),
            "s3" => Ok(SyncBackendArg::S3),
            "webdav" => Ok(SyncBackendArg::Webdav),
            other => Err(format!(
                "invalid BSJ_SYNC_BACKEND '{other}'; expected folder, s3, or webdav"
            )),
        };
    }

    if let Some(remote_arg) = remote_arg {
        if sync::looks_like_s3_remote(remote_arg) {
            return Ok(SyncBackendArg::S3);
        }
        if sync::looks_like_webdav_remote(remote_arg) {
            return Ok(SyncBackendArg::Webdav);
        }
    }

    Ok(SyncBackendArg::Folder)
}

fn resolve_folder_sync_target_path(
    config: &mut config::AppConfig,
    remote_arg: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(remote_arg) = remote_arg {
        let remote_root = config::expand_path_like(remote_arg);
        config.sync_target_path = Some(remote_root.clone());
        config
            .save()
            .map_err(|error| format!("failed to save sync target: {error}"))?;
        return Ok(remote_root);
    }

    config
        .sync_target_path
        .clone()
        .ok_or_else(|| "missing sync target; use --remote PATH".to_string())
}

fn fallback_config_path(app_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
        .join(app_name)
}

fn load_config_state() -> LoadedConfigState {
    let path = config::config_file_path()
        .unwrap_or_else(|_| fallback_config_path("bsj").join("config.json"));
    match config::AppConfig::load() {
        Ok(Some(config)) => LoadedConfigState {
            path,
            config,
            exists: true,
            error: None,
        },
        Ok(None) => LoadedConfigState {
            path,
            config: config::AppConfig::load_or_default(),
            exists: false,
            error: None,
        },
        Err(error) => LoadedConfigState {
            path,
            config: config::AppConfig::load_or_default(),
            exists: true,
            error: Some(error.to_string()),
        },
    }
}

fn load_vault_metadata_state(
    config: &config::AppConfig,
) -> (Option<vault::VaultMetadata>, Option<String>) {
    if !vault::vault_exists(&config.vault_path) {
        return (None, None);
    }
    match vault::load_vault_metadata(&config.vault_path) {
        Ok(metadata) => (Some(metadata), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

fn render_markdown_export(entry: &vault::ExportedEntry) -> String {
    let mut out = String::new();
    out.push_str("# BlueScreen Journal Entry\n\n");
    out.push_str(&format!("Date: {}\n", entry.date.format("%Y-%m-%d")));
    out.push_str(&format!("Entry No.: {}\n\n", entry.entry_number));
    out.push_str(entry.body.trim_end());
    if let Some(closing_thought) = entry.closing_thought.as_deref() {
        if !entry.body.trim_end().is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("## Closing Thought\n\n");
        out.push_str(closing_thought);
    }
    out
}

fn write_plaintext_output(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create output directory: {error}"))?;
    }
    fs::write(path, text).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::render_markdown_export;
    use crate::vault::ExportedEntry;
    use chrono::NaiveDate;

    #[test]
    fn markdown_export_includes_metadata_and_closing_thought() {
        let entry = ExportedEntry {
            date: NaiveDate::from_ymd_opt(2026, 3, 17).expect("date"),
            entry_number: "0000017".to_string(),
            body: "Body text".to_string(),
            closing_thought: Some("Lights out.".to_string()),
        };

        let markdown = render_markdown_export(&entry);
        assert!(markdown.contains("# BlueScreen Journal Entry"));
        assert!(markdown.contains("Entry No.: 0000017"));
        assert!(markdown.contains("## Closing Thought"));
        assert!(markdown.contains("Lights out."));
    }
}
