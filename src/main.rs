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
    if !vault::vault_exists(&config.vault_path) {
        return Err(format!(
            "vault not found at {}",
            config.vault_path.display()
        ));
    }

    let passphrase = match env::var("BSJ_PASSPHRASE") {
        Ok(passphrase) => passphrase,
        Err(_) => rpassword::prompt_password("Vault passphrase: ")
            .map_err(|error| format!("failed to read passphrase: {error}"))?,
    };
    let secret = SecretString::new(passphrase.into_boxed_str());

    let vault = vault::unlock_vault(&config.vault_path, &secret)
        .map_err(|error| format!("unlock failed: {error}"))?;
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
