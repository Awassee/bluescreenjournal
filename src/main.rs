mod config;
mod search;
mod tui;
mod vault;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};
use secrecy::SecretString;
use std::env;

#[derive(Debug, Parser)]
#[command(name = "bsj", about = "BlueScreen Journal")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open a specific journal date in the TUI
    Open { date: String },
    /// Search encrypted entries without writing a plaintext index
    Search {
        query: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    /// Sync encrypted revisions with a folder target
    Sync {
        #[arg(long)]
        remote: Option<String>,
    },
    /// Verify revision hashchains
    Verify,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Open { date }) => {
            let initial_date = match parse_date_arg("date", &date) {
                Ok(date) => Some(date),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(2);
                }
            };

            if let Err(error) = tui::run(initial_date) {
                eprintln!("failed to launch TUI: {error}");
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
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Sync { remote }) => {
            if let Err(error) = run_cli_sync(remote.as_deref()) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Some(Command::Verify) => {
            if let Err(error) = run_cli_verify() {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        None => {
            if let Err(error) = tui::run(None) {
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

fn run_cli_sync(remote_arg: Option<&str>) -> Result<(), String> {
    let mut config = config::AppConfig::load_or_default();
    let remote_root = resolve_sync_target_path(&mut config, remote_arg)?;
    let vault = unlock_cli_vault(&config)?;
    let report = vault
        .sync_folder(&remote_root)
        .map_err(|error| format!("sync failed: {error}"))?;

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

fn resolve_sync_target_path(
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
