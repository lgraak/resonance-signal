//! Durable, privacy-bounded diagnostics for the Windows beta runtime.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const LOG_FILE_NAME: &str = "resonance-signal.log";
const MAX_LOG_FILE_BYTES: u64 = 1_048_576;
const MAX_LOG_MESSAGE_BYTES: usize = 4_096;
const ROTATED_LOG_FILES: usize = 2;

static DIAGNOSTICS: OnceLock<Diagnostics> = OnceLock::new();
static PANIC_HOOK: OnceLock<()> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Info,
    Debug,
}

impl LogLevel {
    fn as_u8(self) -> u8 {
        match self {
            Self::Info => 0,
            Self::Debug => 1,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Debug,
            _ => Self::Info,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Debug => "Debug",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Preferences {
    log_level: LogLevel,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
        }
    }
}

struct Diagnostics {
    paths: DiagnosticsPaths,
    level: std::sync::atomic::AtomicU8,
    lifecycle_state: Mutex<String>,
    writer: Mutex<()>,
    max_log_file_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiagnosticsPaths {
    directory: PathBuf,
    log_file: PathBuf,
    preferences_file: PathBuf,
}

impl DiagnosticsPaths {
    fn from_local_app_data(local_app_data: &Path) -> Self {
        let app = local_app_data.join("Resonance Signal");
        let directory = app.join("logs");
        Self {
            log_file: directory.join(LOG_FILE_NAME),
            preferences_file: app.join("settings.json"),
            directory,
        }
    }
}

pub fn initialize(runtime_mode: &str) -> Result<LogLevel, String> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is unavailable for durable diagnostics".to_string())?;
    let paths = DiagnosticsPaths::from_local_app_data(&local_app_data);
    fs::create_dir_all(&paths.directory)
        .map_err(|error| format!("failed to create diagnostics directory: {error}"))?;
    let (level, preference_warning) = load_preferences(&paths.preferences_file);
    let diagnostics = Diagnostics {
        paths,
        level: std::sync::atomic::AtomicU8::new(level.as_u8()),
        lifecycle_state: Mutex::new("process_initializing".to_string()),
        writer: Mutex::new(()),
        max_log_file_bytes: MAX_LOG_FILE_BYTES,
    };
    DIAGNOSTICS
        .set(diagnostics)
        .map_err(|_| "diagnostics are already initialized".to_string())?;
    install_panic_hook(runtime_mode);
    info(&format!(
        "application_start version={} process_id={} runtime_mode={} protocol_version=1 log_level={}",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        sanitize_field(runtime_mode),
        level.label().to_ascii_lowercase()
    ));
    if let Some(warning) = preference_warning {
        info(&format!(
            "diagnostic_preference_fallback level=info reason={}",
            sanitize_field(&warning)
        ));
    }
    Ok(level)
}

pub fn current_level() -> LogLevel {
    DIAGNOSTICS
        .get()
        .map(|diagnostics| {
            LogLevel::from_u8(diagnostics.level.load(std::sync::atomic::Ordering::Acquire))
        })
        .unwrap_or_default()
}

pub fn set_level(level: LogLevel) -> Result<(), String> {
    let diagnostics = DIAGNOSTICS
        .get()
        .ok_or_else(|| "diagnostics are not initialized".to_string())?;
    persist_preferences(&diagnostics.paths.preferences_file, level)?;
    let previous = diagnostics
        .level
        .swap(level.as_u8(), std::sync::atomic::Ordering::AcqRel);
    diagnostics.write(
        "INFO",
        &format!(
            "log_level_changed previous={} current={}",
            LogLevel::from_u8(previous).label().to_ascii_lowercase(),
            level.label().to_ascii_lowercase()
        ),
    );
    Ok(())
}

pub fn directory() -> Result<PathBuf, String> {
    let diagnostics = DIAGNOSTICS
        .get()
        .ok_or_else(|| "diagnostics are not initialized".to_string())?;
    fs::create_dir_all(&diagnostics.paths.directory)
        .map_err(|error| format!("failed to create diagnostics directory: {error}"))?;
    Ok(diagnostics.paths.directory.clone())
}

pub fn info(message: &str) {
    if let Some(diagnostics) = DIAGNOSTICS.get() {
        diagnostics.write("INFO", message);
    }
}

pub fn debug(message: &str) {
    if let Some(diagnostics) = DIAGNOSTICS.get() {
        if current_level() == LogLevel::Debug {
            diagnostics.write("DEBUG", message);
        }
    }
}

pub fn set_lifecycle_state(state: &str) {
    if let Some(diagnostics) = DIAGNOSTICS.get() {
        *diagnostics
            .lifecycle_state
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = sanitize_field(state);
        debug(&format!("lifecycle_state state={}", sanitize_field(state)));
    }
}

pub fn orderly_exit(reason: &str) {
    set_lifecycle_state("process_exiting_orderly");
    info(&orderly_exit_message(reason));
}

pub fn unexpected_exit(reason: &str, _message: &str) {
    set_lifecycle_state("process_exiting_with_error");
    info(&unexpected_exit_message(reason));
}

fn orderly_exit_message(reason: &str) -> String {
    format!(
        "process_exit orderly=true reason={}",
        sanitize_field(reason)
    )
}

fn unexpected_exit_message(reason: &str) -> String {
    format!(
        "process_exit orderly=false reason={} detail=omitted_by_privacy_boundary",
        sanitize_field(reason)
    )
}

fn install_panic_hook(runtime_mode: &str) {
    let runtime_mode = runtime_mode.to_string();
    let report_to_console = runtime_mode != "tray";
    let _ = PANIC_HOOK.set(()).map(|()| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            record_panic(panic, &runtime_mode);
            if report_to_console {
                previous(panic);
            }
        }));
    });
}

fn record_panic(panic: &PanicHookInfo<'_>, runtime_mode: &str) {
    let Some(diagnostics) = DIAGNOSTICS.get() else {
        return;
    };
    let message = panic_message(panic.payload());
    let location = panic
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "unavailable".to_string());
    let state = diagnostics
        .lifecycle_state
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    diagnostics.write(
        "INFO",
        &format!(
            "panic version={} process_id={} runtime_mode={} lifecycle_state={} location={} message={} backtrace={}",
            env!("CARGO_PKG_VERSION"),
            std::process::id(),
            sanitize_field(runtime_mode),
            sanitize_field(&state),
            sanitize_field(&location),
            sanitize_field(&message),
            sanitize_field(&Backtrace::force_capture().to_string())
        ),
    );
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

fn load_preferences(path: &Path) -> (LogLevel, Option<String>) {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
            Ok(preferences) => (preferences.log_level, None),
            Err(error) => (
                LogLevel::Info,
                Some(format!("malformed settings ignored: {error}")),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (LogLevel::Info, None),
        Err(error) => (
            LogLevel::Info,
            Some(format!("settings could not be read: {error}")),
        ),
    }
}

fn persist_preferences(path: &Path, level: LogLevel) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "diagnostic settings path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create diagnostic settings directory: {error}"))?;
    let contents = serde_json::to_vec_pretty(&Preferences { log_level: level })
        .map_err(|error| format!("failed to encode diagnostic settings: {error}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|error| format!("failed to open diagnostic settings: {error}"))?;
    file.write_all(&contents)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("failed to persist diagnostic settings: {error}"))
}

impl Diagnostics {
    fn write(&self, level: &str, message: &str) {
        let _guard = self
            .writer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let line = format_log_line(level, message);
        if rotate_if_needed(
            &self.paths.log_file,
            self.max_log_file_bytes,
            line.len() as u64,
        )
        .is_err()
        {
            return;
        }
        let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.paths.log_file)
        else {
            return;
        };
        if file.write_all(line.as_bytes()).is_ok() {
            let _ = file.sync_data();
        }
    }
}

fn format_log_line(level: &str, message: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    format!(
        "timestamp_unix_ms={timestamp} level={level} {}\n",
        sanitize_message(message)
    )
}

fn sanitize_message(message: &str) -> String {
    let mut sanitized = message.replace(['\r', '\n', '\0'], " ");
    if sanitized.len() > MAX_LOG_MESSAGE_BYTES {
        let mut boundary = MAX_LOG_MESSAGE_BYTES;
        while !sanitized.is_char_boundary(boundary) {
            boundary -= 1;
        }
        sanitized.truncate(boundary);
        sanitized.push_str(" [truncated]");
    }
    sanitized
}

fn sanitize_field(value: &str) -> String {
    sanitize_message(value)
        .chars()
        .map(|character| {
            if character.is_whitespace() {
                '_'
            } else {
                character
            }
        })
        .collect()
}

fn rotate_if_needed(path: &Path, limit: u64, incoming_bytes: u64) -> std::io::Result<()> {
    let length = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    if length == 0 || length.saturating_add(incoming_bytes) <= limit {
        return Ok(());
    }
    for index in (1..=ROTATED_LOG_FILES).rev() {
        let destination = rotated_path(path, index);
        if index == ROTATED_LOG_FILES && destination.exists() {
            fs::remove_file(&destination)?;
        }
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            rotated_path(path, index - 1)
        };
        if source.exists() {
            fs::rename(source, destination)?;
        }
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    path.with_file_name(format!("resonance-signal.{index}.log"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "resonance-signal-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn log_level_defaults_and_parses_both_supported_values() {
        let missing = TempDirectory::new("missing-settings");
        assert_eq!(
            load_preferences(&missing.0.join("settings.json")),
            (LogLevel::Info, None)
        );

        let path = missing.0.join("settings.json");
        fs::write(&path, br#"{"log_level":"debug"}"#).unwrap();
        assert_eq!(load_preferences(&path), (LogLevel::Debug, None));
        fs::write(&path, br#"{"log_level":"info"}"#).unwrap();
        assert_eq!(load_preferences(&path), (LogLevel::Info, None));
    }

    #[test]
    fn preferences_persist_and_malformed_settings_fall_back_to_info() {
        let temp = TempDirectory::new("settings-round-trip");
        let path = temp.0.join("settings.json");
        persist_preferences(&path, LogLevel::Debug).unwrap();
        assert_eq!(load_preferences(&path), (LogLevel::Debug, None));

        fs::write(&path, b"not-json").unwrap();
        let (level, warning) = load_preferences(&path);
        assert_eq!(level, LogLevel::Info);
        assert!(warning.unwrap().contains("malformed settings ignored"));
    }

    #[test]
    fn rotation_retains_two_bounded_history_files() {
        let temp = TempDirectory::new("rotation");
        let path = temp.0.join(LOG_FILE_NAME);
        for marker in ["first", "second", "third", "fourth"] {
            let line = format!("{marker}-1234567890\n");
            rotate_if_needed(&path, 20, line.len() as u64).unwrap();
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            file.write_all(line.as_bytes()).unwrap();
        }
        assert!(path.metadata().unwrap().len() <= 20);
        assert!(rotated_path(&path, 1).metadata().unwrap().len() <= 20);
        assert!(rotated_path(&path, 2).metadata().unwrap().len() <= 20);
        assert!(fs::read_to_string(rotated_path(&path, 2))
            .unwrap()
            .contains("second"));
    }

    #[test]
    fn panic_formatting_is_single_line_and_bounded() {
        let line = format_log_line(
            "INFO",
            &format!(
                "panic message={} backtrace={}",
                "failure\r\n",
                "x".repeat(8_000)
            ),
        );
        assert_eq!(line.lines().count(), 1);
        assert!(line.contains("panic message=failure  "));
        assert!(line.contains("[truncated]"));
    }

    #[test]
    fn diagnostics_paths_are_per_user_and_outside_the_repository() {
        let paths =
            DiagnosticsPaths::from_local_app_data(Path::new(r"C:\Users\Tester\AppData\Local"));
        assert!(paths
            .log_file
            .ends_with(r"Resonance Signal\logs\resonance-signal.log"));
        assert!(paths
            .preferences_file
            .ends_with(r"Resonance Signal\settings.json"));
    }

    #[test]
    fn exit_messages_distinguish_orderly_and_unexpected_shutdown() {
        assert_eq!(
            orderly_exit_message("tray_exit"),
            "process_exit orderly=true reason=tray_exit"
        );
        assert_eq!(
            unexpected_exit_message("runtime error"),
            "process_exit orderly=false reason=runtime_error detail=omitted_by_privacy_boundary"
        );
    }
}
