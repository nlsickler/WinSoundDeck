use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
        }
    }
}

struct LoggerState {
    component: &'static str,
    max_level: LogLevel,
    log_path: PathBuf,
}

static LOGGER: OnceLock<Mutex<LoggerState>> = OnceLock::new();

pub fn default_log_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata)
            .join("winsounddeck")
            .join("logs")
            .join("winsounddeck.log")
    } else {
        PathBuf::from("winsounddeck.log")
    }
}

/// Initializes process-wide logging (`DEBUG` in debug builds, `WARN` in release builds).
pub fn init_logging(component: &'static str) {
    let max_level = if cfg!(debug_assertions) {
        LogLevel::Debug
    } else {
        LogLevel::Warn
    };
    let log_path = default_log_path();
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = LOGGER.set(Mutex::new(LoggerState {
        component,
        max_level,
        log_path,
    }));
}

pub fn write_log(level: LogLevel, msg: &str) {
    let Some(lock) = LOGGER.get() else {
        return;
    };
    let Ok(guard) = lock.lock() else {
        return;
    };
    if level > guard.max_level {
        return;
    }

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("[{secs}] [{}] [{}] {msg}\n", level.as_str(), guard.component);

    eprint!("{line}");

    if let Ok(meta) = fs::metadata(&guard.log_path) {
        if meta.len() > 2 * 1024 * 1024 {
            let _ = fs::write(&guard.log_path, b"");
        }
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&guard.log_path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        $crate::logging::write_log($crate::logging::LogLevel::Error, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        $crate::logging::write_log($crate::logging::LogLevel::Warn, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        $crate::logging::write_log($crate::logging::LogLevel::Info, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        $crate::logging::write_log($crate::logging::LogLevel::Debug, &format!($($arg)*))
    };
}
