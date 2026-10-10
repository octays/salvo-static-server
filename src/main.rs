mod args;
mod cache;
mod logging;
mod middleware;
mod router;
mod server;
mod static_files;

#[cfg(test)]
mod tests;

use args::Args;
use clap::Parser;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    logging::initialize_logging(&args.log_level);
    server::run(args).await;
}
