use salvo::__private::tracing;

pub(crate) fn initialize_logging(log_level: &str) {
    let level = match log_level.to_lowercase().as_str() {
        "trace" => Some(tracing::Level::TRACE),
        "debug" => Some(tracing::Level::DEBUG),
        "info" => Some(tracing::Level::INFO),
        "warn" => Some(tracing::Level::WARN),
        "error" => Some(tracing::Level::ERROR),
        _ => None,
    };

    tracing_subscriber::fmt()
        .with_max_level(level.unwrap_or(tracing::Level::INFO))
        .with_target(false)
        .init();
    if level.is_none() {
        tracing::warn!(log_level, "Invalid log level; falling back to 'info'");
    }
}
