mod config;
mod tui;
mod vault;

fn main() {
    if let Err(error) = tui::run() {
        eprintln!("failed to launch TUI: {error}");
        std::process::exit(1);
    }
}
