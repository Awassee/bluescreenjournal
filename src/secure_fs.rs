use rand::{RngCore, rngs::OsRng};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;

pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    let missing_directories = missing_directory_chain(path);
    fs::create_dir_all(path)?;
    for directory in missing_directories.into_iter().rev() {
        set_private_dir_permissions(&directory)?;
    }
    set_private_dir_permissions(path)
}

pub fn open_private_log_file(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }

    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);

    let file = options.open(path)?;
    set_private_file_permissions(path)?;
    Ok(file)
}

pub fn atomic_write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    ensure_private_dir(parent)?;
    atomic_write_with_mode(path, bytes)
}

pub fn atomic_write_restricted(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    fs::create_dir_all(parent)?;
    atomic_write_with_mode(path, bytes)
}

pub fn set_private_file_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    }
    Ok(())
}

pub fn set_private_dir_permissions(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))?;
    }
    Ok(())
}

pub fn sync_dir(path: &Path) -> io::Result<()> {
    let dir = File::open(path)?;
    dir.sync_all()
}

fn atomic_write_with_mode(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent directory"))?;
    let tmp_path = unique_temp_path(parent, path.file_name().and_then(|name| name.to_str()));

    let write_result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        options.mode(PRIVATE_FILE_MODE);

        let mut file = options.open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp_path, path)?;
        set_private_file_permissions(path)?;
        sync_dir(parent)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }

    write_result
}

fn unique_temp_path(parent: &Path, file_name: Option<&str>) -> PathBuf {
    let mut suffix = [0u8; 6];
    OsRng.fill_bytes(&mut suffix);
    parent.join(format!(
        ".{}.tmp-{}",
        file_name.unwrap_or("tmp"),
        hex::encode(suffix)
    ))
}

fn missing_directory_chain(path: &Path) -> Vec<PathBuf> {
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        missing.push(cursor.to_path_buf());
        match cursor.parent() {
            Some(parent) if parent != cursor => cursor = parent,
            _ => break,
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::{atomic_write_private, atomic_write_restricted, ensure_private_dir};
    use tempfile::tempdir;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn atomic_write_restricted_creates_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("notes").join("entry.txt");
        atomic_write_restricted(&path, b"hello").expect("write");
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "hello");
    }

    #[test]
    fn ensure_private_dir_creates_directory() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("vault").join("entries");
        ensure_private_dir(&path).expect("dir");
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_private_uses_private_file_mode() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("vault.json");
        atomic_write_private(&path, b"{}").expect("write");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_private_dir_uses_private_directory_mode() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("vault");
        ensure_private_dir(&path).expect("dir");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_private_dir_secures_new_intermediate_directories() {
        let dir = tempdir().expect("tempdir");
        let year = dir.path().join("vault").join("entries").join("2026");
        let date = year.join("2026-03-17");
        ensure_private_dir(&date).expect("dir");
        let year_mode = std::fs::metadata(&year)
            .expect("year meta")
            .permissions()
            .mode()
            & 0o777;
        let date_mode = std::fs::metadata(&date)
            .expect("date meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(year_mode, 0o700);
        assert_eq!(date_mode, 0o700);
    }
}
