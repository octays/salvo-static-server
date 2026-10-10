use clap::Parser;
use std::net::SocketAddr;

/// Static server - A Rust-based static file server
#[derive(Parser, Debug)]
#[command(name = "static-server")]
#[command(author = "Rust Static Server Team")]
#[command(version)]
#[command(about = "Rust static file server with automatic certificate management", long_about = None)]
pub(crate) struct Args {
    /// HTTP listen address
    #[arg(long, default_value = "0.0.0.0:8080")]
    pub(crate) http_addr: SocketAddr,

    /// HTTPS listen address
    #[arg(long, default_value = "0.0.0.0:8443")]
    pub(crate) https_addr: SocketAddr,

    /// Enable HTTP/3
    #[arg(long, requires = "https_enable")]
    pub(crate) http3_enable: bool,

    /// Enable HTTPS
    #[arg(long, requires = "acme_domain")]
    pub(crate) https_enable: bool,

    /// Redirect HTTP requests to HTTPS
    #[arg(long, requires = "https_enable")]
    pub(crate) force_https: bool,

    /// External HTTPS redirect port (defaults to the HTTPS listen port)
    #[arg(long, requires = "force_https", value_parser = clap::value_parser!(u16).range(1..))]
    pub(crate) https_redirect_port: Option<u16>,

    /// Explicitly enable response compression (enabled by default)
    #[arg(long, conflicts_with = "no_compression")]
    pub(crate) compression: bool,

    /// Disable dynamic response compression
    #[arg(long)]
    pub(crate) no_compression: bool,

    /// Domain for automatic ACME certificate issuance
    #[arg(long)]
    pub(crate) acme_domain: Option<String>,

    /// Static file directories
    #[arg(long, default_values = &["static"])]
    pub(crate) static_dir: Vec<String>,

    /// Default filename for directory requests
    #[arg(long, default_value = "index.html")]
    pub(crate) try_file: String,

    /// Fallback page when the requested file is not found
    #[arg(long, default_value = "index.html")]
    pub(crate) default_page: String,

    /// Additional filenames requiring Cache-Control: no-cache (comma-separated, exact matches)
    /// Example: --no-cache-patterns index.html,config.json
    #[arg(long, value_delimiter = ',', default_values = &["index.html"])]
    pub(crate) no_cache_patterns: Vec<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub(crate) log_level: String,
}
