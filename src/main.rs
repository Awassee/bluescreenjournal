mod config;
mod tui;
mod vault;

use chrono::NaiveDate;
use clap::{Parser, Subcommand};

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
}

fn main() {
    let cli = Cli::parse();
    let initial_date = match cli.command {
        Some(Command::Open { date }) => match NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
            Ok(date) => Some(date),
            Err(_) => {
                eprintln!("invalid date '{date}'; expected YYYY-MM-DD");
                std::process::exit(2);
            }
        },
        None => None,
    };

    if let Err(error) = tui::run(initial_date) {
        eprintln!("failed to launch TUI: {error}");
        std::process::exit(1);
    }
}
