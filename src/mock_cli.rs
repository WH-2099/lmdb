use std::{io::Write, net::IpAddr, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use model_metadata::mock::{config, server, session};
use serde_json::json;

#[derive(Subcommand)]
pub enum Command {
    /// Serve a selected provider's model APIs without contacting that provider.
    Serve(Serve),
    /// List configured models, protocol coverage and supporting sources.
    List {
        #[arg(long)]
        provider: Option<String>,
        /// Complete JSON or YAML configuration; defaults are embedded in the binary.
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Describe scenarios and the requests needed to trigger them.
    Scenarios,
    /// Show the captured Intelligence Index ranks and verified API bindings.
    Rankings {
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Check a configuration without opening a socket.
    Validate {
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Suite {
    Normal,
    Coverage,
}

#[derive(Args)]
pub struct Serve {
    #[arg(long)]
    provider: String,
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    /// Port 0 lets the OS choose a free port; the first stdout line reports it.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    #[arg(long)]
    config: Option<PathBuf>,
    /// Select exact API model IDs from this provider's configured models.
    #[arg(long = "model", value_delimiter = ',')]
    models: Vec<String>,
    #[arg(long, value_enum, default_value = "normal")]
    suite: Suite,
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// A local fake credential for exercising client authentication.
    #[arg(long, default_value = "mock-key", conflicts_with = "no_auth")]
    api_key: String,
    #[arg(long)]
    no_auth: bool,
    /// Delay before the response starts, in milliseconds.
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,
    /// Delay between stream chunks, in milliseconds.
    #[arg(long, default_value_t = 0)]
    chunk_delay_ms: u64,
    /// Split SSE bytes at this size, including inside UTF-8; 0 preserves frames.
    #[arg(long, default_value_t = 0)]
    chunk_bytes: usize,
    #[arg(long, default_value_t = 2 * 1024 * 1024)]
    max_body_bytes: usize,
    #[arg(long, default_value_t = 1024)]
    max_sessions: usize,
    #[arg(long, default_value_t = 1800)]
    session_ttl_secs: u64,
    #[arg(long, default_value_t = 64)]
    max_inflight: usize,
    /// Deadline for reading a request body; intentional response delays are separate.
    #[arg(long, default_value_t = 10000)]
    request_timeout_ms: u64,
    /// Optional URL prefix, for example /test, before native API and admin routes.
    #[arg(long, default_value = "")]
    base_path: String,
}

pub async fn run(command: Command) -> Result<()> {
    match command {
        Command::Serve(args) => {
            let config = config::load(args.config.as_deref())?;
            let provider = config::provider(&config, &args.provider)?.id.clone();
            let base_path = args.base_path.trim_end_matches('/').to_owned();
            let options = server::Options {
                provider: provider.clone(),
                models: args.models,
                api_key: (!args.no_auth).then_some(args.api_key),
                suite: match args.suite {
                    Suite::Normal => session::Suite::Normal,
                    Suite::Coverage => session::Suite::Coverage,
                },
                seed: args.seed,
                delay_ms: args.delay_ms,
                chunk_delay_ms: args.chunk_delay_ms,
                chunk_bytes: args.chunk_bytes,
                max_body_bytes: args.max_body_bytes,
                max_sessions: args.max_sessions,
                session_ttl_secs: args.session_ttl_secs,
                max_inflight: args.max_inflight,
                request_timeout_ms: args.request_timeout_ms,
                base_path: base_path.clone(),
            };
            let router = server::router(config, options)?;
            let listener = tokio::net::TcpListener::bind((args.host, args.port))
                .await
                .with_context(|| format!("bind {}:{}", args.host, args.port))?;
            let address = listener.local_addr()?;
            println!(
                "{}",
                json!({"listening":format!("http://{address}{base_path}"),"provider":provider,"simulated":true,"seed":args.seed})
            );
            std::io::stdout().flush()?;
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::List { provider, config } => {
            let config = config::load(config.as_deref())?;
            if let Some(id) = provider {
                super::print_json(
                    &json!({"provider":config::provider(&config, &id)?,"sources":config.sources}),
                )?;
            } else {
                super::print_json(&config)?;
            }
        }
        Command::Scenarios => super::print_json(&session::plan())?,
        Command::Rankings { top } => super::print_json(&config::rankings(top)?)?,
        Command::Validate { config } => {
            let config = config::load(config.as_deref())?;
            super::print_json(&json!({
                "valid":true,
                "providers":config.providers.len(),
                "models":config.providers.iter().map(|provider|provider.models.len()).sum::<usize>(),
                "script_steps":config.script.as_ref().map_or(0,|script|script.steps.len())
            }))?;
        }
    }
    Ok(())
}
