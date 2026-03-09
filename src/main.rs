use clap::{Parser, Subcommand};
use http::Uri;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(name = "wirejack")]
#[command(about = "Lightweight TLS terminating HTTP proxy", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Http {
        /// Path to a python handler script
        handler: PathBuf,

        /// Proxy listener bind address
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        bind: SocketAddr,

        /// Upstream proxy
        #[arg(short, long)]
        proxy: Option<Uri>,

        /// Whitelist of domains to intercept
        #[arg(short, long)]
        filter: Vec<Uri>,

        /// Start a python interpreter to interact with the running proxy
        #[arg(short, long)]
        interactive: bool,

        /// Set the number of HTTP handler threads
        #[arg(short, long, default_value = "4")]
        threads: usize,
    },
}

fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::filter::LevelFilter::INFO)
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=trace,tower_http=debug", env!("CARGO_CRATE_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Cli::parse();
    match args.command {
        Command::Http {
            handler,
            bind,
            proxy,
            filter,
            interactive,
            threads,
        } => {
            let config = wirejack::HttpConfig {
                handler,
                bind: vec![bind],
                proxy,
                filter,
                interactive,
                threads,
            };
            wirejack::proxy_http(config);
        }
    }
}
