mod config;
mod logging;
mod search;
mod sync;
mod tui;
mod vault;

use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use secrecy::SecretString;
use std::env;

#[derive(Debug, Parser)]
#[command(
    name = "bsj",
    about = "BlueScreen Journal",
    long_about = "BlueScreen Journal is an encrypted, local-first macOS terminal journal with a nostalgic blue-screen full-screen editor, append-only revisions, encrypted drafts, and encrypted sync targets.",
    after_help = "Examples:\n  bsj\n  bsj open 2026-03-16\n  bsj search \"quiet morning\" --from 2026-03-01 --to 2026-03-31\n  bsj export 2026-03-16\n  bsj sync --backend folder --remote ~/Documents/BlueScreenJournal-Sync\n  bsj backup\n  bsj restore ~/Documents/BlueScreenJournal/vault/backups/backup-20260316T120000Z.bsjbak.enc --into ~/Documents/BlueScreenJournal-Restore\n\nDebug logging:\n  Use --debug to enable verbose file logging at ~/Library/Logs/bsj/bsj.log"
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
    /// Export a date as plain text to stdout
    Export { date: String },
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
    /// Create an encrypted backup snapshot under vault/backups
    Backup,
    /// Restore an encrypted backup snapshot into a target directory
    Restore {
        backup: String,
        #[arg(long)]
        into: String,
    },
    /// Verify revision hashchains
    Verify,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum SyncBackendArg {
    Folder,
    S3,
    Webdav,
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
        Some(Command::Export { date }) => {
            let date = match parse_date_arg("date", &date) {
                Ok(date) => date,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = run_cli_export(date) {
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
        Some(Command::Backup) => {
            if let Err(error) = run_cli_backup() {
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

fn run_cli_export(date: NaiveDate) -> Result<(), String> {
    log::info!("running CLI export");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let Some(text) = vault
        .export_entry_text(date)
        .map_err(|error| format!("export failed: {error}"))?
    else {
        return Err(format!("no saved entry for {}", date.format("%Y-%m-%d")));
    };
    println!("{text}");
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

fn run_cli_backup() -> Result<(), String> {
    log::info!("running CLI backup");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let summary = vault
        .create_backup(&config.backup_retention)
        .map_err(|error| format!("backup failed: {error}"))?;
    println!("Backup: {}", summary.path.display());
    println!("Pruned: {}", summary.pruned);
    Ok(())
}

fn run_cli_restore(backup: &str, into: &str) -> Result<(), String> {
    log::info!("running CLI restore");
    let config = config::AppConfig::load_or_default();
    let vault = unlock_cli_vault(&config)?;
    let backup_path = expand_tilde(backup);
    let restore_root = expand_tilde(into);
    vault
        .restore_backup_into(&backup_path, &restore_root)
        .map_err(|error| format!("restore failed: {error}"))?;
    println!("Restored: {}", restore_root.display());
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
) -> Result<std::path::PathBuf, String> {
    if let Some(remote_arg) = remote_arg {
        let remote_root = expand_tilde(remote_arg);
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

fn expand_tilde(input: &str) -> std::path::PathBuf {
    if input == "~" {
        return dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(input));
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    std::path::PathBuf::from(input)
}
