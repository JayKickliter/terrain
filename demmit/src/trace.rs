//! Tracing setup: a rolling daily log in the OS log directory.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initializes logging to stdout.
pub fn to_stdout() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,demmit=debug"));
    let stdout_log = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_log)
        .init();
}
