use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

/// Replace a file with a fully written, private sibling in the same directory.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected a file path"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut temp_name = name.to_os_string();
    temp_name.push(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let temp_path = path.with_file_name(temp_name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temp_path)?;
    let result = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);

    let result = result.and_then(|()| fs::rename(&temp_path, path));
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_replacement_removes_temporary_file() {
        let dir = std::env::temp_dir().join(format!("cx-atomic-{}", uuid::Uuid::new_v4()));
        let path = dir.join("state.json");
        // A directory at the destination prevents the final rename.
        fs::create_dir_all(&path).unwrap();
        let result = write(&path, b"replacement");
        let entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert!(path.is_dir());
        fs::remove_dir_all(dir).unwrap();

        assert!(result.is_err());
        assert_eq!(entries, [path]);
    }
}
