// AGPL-3.0 许可证

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::collections::VecDeque;
use std::fmt::Write as FmtWrite;
use std::os::windows::io::FromRawHandle;

use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::registry::LookupSpan;

#[cfg(windows)]
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL,
    FILE_FLAG_WRITE_THROUGH,
};
#[cfg(windows)]
use windows::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::ffi::OsStr;

const LOG_DIR_NAME: &str = "AkiSpace\\logs";
const LOG_PREFIXES: &[&str] = &["akispace", "akispace-fixenv", "akispace-agent"];
const PRUNE_DAYS: u64 = 14;
const FLUSH_INTERVAL_MS: u64 = 100;
const MAX_BUFFER_LINES: usize = 1000;

static WRITER_HANDLE: OnceLock<Arc<LogWriter>> = OnceLock::new();

fn log_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(LOG_DIR_NAME)
}

fn ensure_log_dir() {
    let _ = fs::create_dir_all(log_dir());
}

fn current_date_str() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day = 86400;
    let days = now / secs_per_day;
    format!("{:04}{:02}{:02}", 1970 + days / 365, (days % 365) / 30 + 1, days % 30 + 1)
}

fn current_time_str() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86400;
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    let millis = now.subsec_millis();
    format!("{:02}:{:02}:{:02}.{:03}", hours, mins, secs, millis)
}

fn log_file_path(prefix: &str) -> PathBuf {
    log_dir().join(format!("{}-{}.log", prefix, current_date_str()))
}

fn open_log_file(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        let wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
        let pcwstr = PCWSTR(wide.as_ptr());
        let handle = unsafe {
            CreateFileW(
                pcwstr,
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_WRITE_THROUGH,
                None,
            )
        };
        match handle {
            Ok(h) if h != INVALID_HANDLE_VALUE => {
                return Ok(unsafe { File::from_raw_handle(h.0 as *mut std::ffi::c_void) });
            }
            _ => {}
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

struct LogWriter {
    buffers: Mutex<[VecDeque<String>; 3]>,
    current_files: Mutex<[Option<BufWriter<File>>; 3]>,
    current_date: Mutex<String>,
    shutdown: Mutex<bool>,
}

impl LogWriter {
    fn new() -> Self {
        ensure_log_dir();
        Self {
            buffers: Mutex::new([VecDeque::new(), VecDeque::new(), VecDeque::new()]),
            current_files: Mutex::new([None, None, None]),
            current_date: Mutex::new(current_date_str()),
            shutdown: Mutex::new(false),
        }
    }

    fn get_or_create_writer(&self, idx: usize) -> Option<BufWriter<File>> {
        let mut files = self.current_files.lock().unwrap();
        let mut date = self.current_date.lock().unwrap();
        let today = current_date_str();

        if *date != today {
            *date = today;
            *files = [None, None, None];
        }

        if files[idx].is_none() {
            let path = log_file_path(LOG_PREFIXES[idx]);
            match open_log_file(&path) {
                Ok(f) => files[idx] = Some(BufWriter::new(f)),
                Err(e) => {
                    eprintln!("打开日志文件失败 {}: {}", path.display(), e);
                    return None;
                }
            }
        }
        files[idx].take()
    }

    fn write_line(&self, idx: usize, line: &str) {
        let mut buffers = self.buffers.lock().unwrap();
        let buf = &mut buffers[idx];
        if buf.len() >= MAX_BUFFER_LINES {
            buf.pop_front();
        }
        buf.push_back(line.to_string());
    }

    fn flush_buffer(&self, idx: usize) {
        let mut buffers = self.buffers.lock().unwrap();
        let buf = &mut buffers[idx];
        if buf.is_empty() {
            return;
        }
        if let Some(mut writer) = self.get_or_create_writer(idx) {
            for line in buf.drain(..) {
                let _ = writeln!(writer, "{}", line);
            }
            let _ = writer.flush();
            let mut files = self.current_files.lock().unwrap();
            files[idx] = Some(writer);
        } else {
            for line in buf.drain(..) {
                eprintln!("{}", line);
            }
        }
    }

    fn flush_all(&self) {
        for i in 0..3 {
            self.flush_buffer(i);
        }
    }

    fn prune_old_logs(&self) {
        let dir = log_dir();
        let now = SystemTime::now();
        let cutoff = now - Duration::from_secs(PRUNE_DAYS * 86400);

        for prefix in LOG_PREFIXES {
            let entries = match fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with(prefix) || !name.ends_with(".log") {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified < cutoff {
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
            }
        }
    }

    fn run_background(&self) {
        let writer = Arc::new(self.clone());
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(FLUSH_INTERVAL_MS));
                if *writer.shutdown.lock().unwrap() {
                    writer.flush_all();
                    break;
                }
                writer.flush_all();
                writer.prune_old_logs();
            }
        });
    }

    fn shutdown(&self) {
        *self.shutdown.lock().unwrap() = true;
    }
}

impl Clone for LogWriter {
    fn clone(&self) -> Self {
        Self {
            buffers: Mutex::new([VecDeque::new(), VecDeque::new(), VecDeque::new()]),
            current_files: Mutex::new([None, None, None]),
            current_date: Mutex::new(current_date_str()),
            shutdown: Mutex::new(false),
        }
    }
}

fn writer() -> Arc<LogWriter> {
    WRITER_HANDLE.get_or_init(|| {
        let w = Arc::new(LogWriter::new());
        w.run_background();
        w
    }).clone()
}

pub fn log_to(prefix: &str, level: Level, category: &str, message: &str) {
    let idx = LOG_PREFIXES.iter().position(|&p| p == prefix).unwrap_or(0);
    let line = format!("{} [{}] {}: {}", current_time_str(), level, category, message);
    writer().write_line(idx, &line);
}

pub fn log_exception(prefix: &str, level: Level, category: &str, message: &str, exception: &str) {
    let idx = LOG_PREFIXES.iter().position(|&p| p == prefix).unwrap_or(0);
    let mut line = format!("{} [{}] {}: {}", current_time_str(), level, category, message);
    line.push_str("\n");
    line.push_str(exception);
    writer().write_line(idx, &line);
}

pub struct LogLayer;

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        let level = *meta.level();
        let category = meta.target();

        let mut message = String::new();
        let mut visitor = MessageVisitor(&mut message);
        event.record(&mut visitor);

        let prefix = match category {
            c if c.contains("fixenv") => "akispace-fixenv",
            c if c.contains("agent") => "akispace-agent",
            _ => "akispace",
        };

        log_to(prefix, level, category, &message);
    }
}

struct MessageVisitor<'a>(&'a mut String);

impl<'a> tracing::field::Visit for MessageVisitor<'a> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.0, "{:?}", value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0.push_str(value);
        }
    }
}

pub fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    use tracing_subscriber::prelude::*;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(LogLayer)
        .with(fmt::layer().with_writer(StderrWriter))
        .init();
}

pub fn init_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = match info.payload().downcast_ref::<&str>() {
            Some(s) => *s,
            None => match info.payload().downcast_ref::<String>() {
                Some(s) => s.as_str(),
                None => "未知 panic",
            },
        };
        let location = info.location().map(|l| l.to_string()).unwrap_or_default();
        log_exception("akispace", Level::ERROR, "panic", &format!("panic: {} at {}", msg, location), "");
    }));
}

struct StderrWriter;

impl MakeWriter<'_> for StderrWriter {
    type Writer = StderrWriterInner;

    fn make_writer(&self) -> Self::Writer {
        StderrWriterInner
    }
}

struct StderrWriterInner;

impl Write for StderrWriterInner {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stderr().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

pub fn shutdown() {
    if let Some(w) = WRITER_HANDLE.get() {
        w.shutdown();
    }
}