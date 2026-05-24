//! Tracing setup + log file housekeeping.

use tracing_appender::non_blocking::WorkerGuard;

const LOG_RETENTION_DAYS: u64 = 7;

/// Delete log files older than `retention_days` in the given directory.
/// Only removes files matching the `allthecodes.log.YYYY-MM-DD` pattern.
fn cleanup_old_logs(log_dir: &std::path::Path, retention_days: u64) {
    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(retention_days * 86400);
    let entries = match std::fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("cc-rust.log.") {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            let modified = meta.modified().unwrap_or(std::time::SystemTime::now());
            if modified < cutoff {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

fn ensure_log_dir_writable(log_dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let probe = log_dir.join(".cc-rust-log-probe");
    let _file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&probe)?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

/// Initialize the dual-layer tracing subscriber (stderr + file), run log
/// housekeeping, and return a guard that must stay alive for the life of
/// the process so the non-blocking file writer can flush on drop.
pub fn init_tracing(verbose: bool) -> WorkerGuard {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;

    // stderr layer respects --verbose / RUST_LOG; file layer always debug.
    let log_level = if verbose { "debug" } else { "warn" };
    let stderr_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    let primary_log_dir = allthecodes_config::paths::logs_dir();
    let log_dir = match ensure_log_dir_writable(&primary_log_dir) {
        Ok(()) => primary_log_dir,
        Err(e) => {
            eprintln!(
                "warning: log directory {} is not writable: {}. Falling back to temp logging.",
                primary_log_dir.display(),
                e
            );
            let fallback = std::env::temp_dir().join("cc-rust").join("logs");
            if let Err(fallback_error) = ensure_log_dir_writable(&fallback) {
                eprintln!(
                    "warning: fallback log directory {} is not writable: {}.",
                    fallback.display(),
                    fallback_error
                );
            }
            fallback
        }
    };
    cleanup_old_logs(&log_dir, LOG_RETENTION_DAYS);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "cc-rust.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .with_filter(stderr_filter),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true)
                .with_line_number(true)
                .with_filter(tracing_subscriber::EnvFilter::new(
                    "debug,reqwest=warn,hyper_util=warn,hyper=warn,h2=warn,rustls=warn,ignore=warn,globset=warn",
                )),
        );

    #[cfg(feature = "telemetry")]
    {
        match allthecodes_services::langfuse::init_langfuse() {
            Ok(Some(tracer)) => {
                let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
                subscriber.with(telemetry).init();
            }
            Ok(None) => subscriber.init(),
            Err(error) => {
                eprintln!(
                    "warning: failed to initialize Langfuse tracing: {}. Continuing without Langfuse.",
                    error
                );
                subscriber.init();
            }
        }
    }

    #[cfg(not(feature = "telemetry"))]
    subscriber.init();

    guard
}
