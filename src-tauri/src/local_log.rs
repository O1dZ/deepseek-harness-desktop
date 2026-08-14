use anyhow::Result;
use chrono::{Duration, Utc};
use parking_lot::Mutex;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone)]
pub struct LocalLog {
    directory: PathBuf,
    path: PathBuf,
    file: Arc<Mutex<File>>,
}

impl LocalLog {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        cleanup_old_logs(&directory)?;
        let path = directory.join("desktop.log");
        rotate_if_needed(&path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;
        Ok(Self {
            directory,
            path,
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn write(&self, source: &str, message: &str) {
        let cleaned = redact(message);
        let stamp = Utc::now().to_rfc3339();
        let mut file = self.file.lock();
        if file
            .metadata()
            .map(|m| m.len() >= MAX_LOG_BYTES)
            .unwrap_or(false)
        {
            let _ = file.set_len(0);
        }
        let _ = writeln!(file, "{stamp} [{source}] {cleaned}");
        let _ = file.flush();
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn clear(&self) -> Result<()> {
        self.file.lock().set_len(0)?;
        for entry in fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if path != self.path && path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
}

fn rotate_if_needed(path: &Path) -> Result<()> {
    if path
        .metadata()
        .map(|m| m.len() >= MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        let rotated = path.with_extension("log.1");
        let _ = fs::remove_file(&rotated);
        fs::rename(path, rotated)?;
    }
    Ok(())
}

fn cleanup_old_logs(directory: &Path) -> Result<()> {
    let cutoff = SystemTime::now() - Duration::days(7).to_std()?;
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_file()
            && path
                .metadata()?
                .modified()
                .map(|time| time < cutoff)
                .unwrap_or(false)
        {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn redact(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if [
        "authorization:",
        "api_key=",
        "api-key=",
        "api key:",
        "bearer ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        "[sensitive runtime output redacted]".to_string()
    } else {
        message.trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_credentials() {
        assert_eq!(
            redact("Authorization: Bearer secret"),
            "[sensitive runtime output redacted]"
        );
        assert_eq!(
            redact("DEEPSEEK_API_KEY=secret"),
            "[sensitive runtime output redacted]"
        );
    }
}
