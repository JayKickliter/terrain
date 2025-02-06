//! Tracing setup: a rolling daily log in the OS log directory.

use camino::Utf8PathBuf;
use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initializes file logging, returning the appender guard.
///
/// The guard must be kept alive for the process lifetime; dropping it
/// stops the background log writer. Returns `None` if no log directory
/// could be determined.
pub fn init() -> Option<WorkerGuard> {
    let dir = log_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let (writer, guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily(&dir, "demmit.log"));
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,demmit=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(writer))
        .init();
    tracing::info!(dir = %dir, "logging started");
    Some(guard)
}

/// OS-appropriate log directory (`<state|data>/logs`).
fn log_dir() -> Option<Utf8PathBuf> {
    let dirs = ProjectDirs::from("xyz", "7r", "demmit")?;
    let base = dirs.state_dir().unwrap_or_else(|| dirs.data_local_dir());
    Utf8PathBuf::from_path_buf(base.join("logs")).ok()
}
