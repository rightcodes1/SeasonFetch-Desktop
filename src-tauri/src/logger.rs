use chrono::Utc;
use seasonfetch_core::validation::redact_sensitive_text;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

pub struct DesktopLogger {
    file_path: PathBuf,
    lock: Mutex<()>,
}

impl DesktopLogger {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            lock: Mutex::new(()),
        }
    }

    fn write(&self, level: &str, scope: &str, message: &str) {
        let _guard = self.lock.lock().ok();
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(meta) = fs::metadata(&self.file_path) {
            if meta.len() >= MAX_LOG_BYTES {
                let backup = self.file_path.with_extension("log.1");
                let _ = fs::remove_file(&backup);
                let _ = fs::rename(&self.file_path, &backup);
            }
        }
        let line = format!(
            "{} [{}] [{}] {}\n",
            Utc::now().to_rfc3339(),
            level,
            scope,
            redact_sensitive_text(message)
        );
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    pub fn info(&self, scope: &str, msg: &str) {
        self.write("INFO", scope, msg);
    }

    pub fn warn(&self, scope: &str, msg: &str) {
        self.write("WARN", scope, msg);
    }

    #[allow(dead_code)]
    pub fn error(&self, scope: &str, msg: &str) {
        self.write("ERROR", scope, msg);
    }

    pub fn read_recent_lines(&self, limit: usize) -> Vec<String> {
        let cap = limit.clamp(1, 500);
        let Ok(raw) = fs::read_to_string(&self.file_path) else {
            return Vec::new();
        };
        let lines: Vec<String> = raw
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(redact_sensitive_text)
            .collect();
        let start = lines.len().saturating_sub(cap);
        lines[start..].to_vec()
    }
}
