use crate::secure_fs;
use log::{LevelFilter, Log, Metadata, Record};
use std::{
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static LOGGER: OnceLock<FileLogger> = OnceLock::new();

pub fn init(debug: bool) -> Result<PathBuf, String> {
    let path = log_file_path();
    let file = secure_fs::open_private_log_file(&path)
        .map_err(|error| format!("failed to open log file: {error}"))?;

    let logger = FileLogger {
        file: Mutex::new(file),
        level: if debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        },
    };

    let logger_ref = LOGGER.get_or_init(|| logger);
    log::set_logger(logger_ref).map_err(|error| format!("failed to set logger: {error}"))?;
    log::set_max_level(logger_ref.level);
    log::info!("logging initialized");
    if debug {
        log::debug!("debug logging enabled");
    }
    Ok(path)
}

pub fn log_file_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Logs")
        .join("bsj")
        .join("bsj.log")
}

struct FileLogger {
    file: Mutex<File>,
    level: LevelFilter,
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{timestamp} {:<5} {}", record.level(), record.args());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}
