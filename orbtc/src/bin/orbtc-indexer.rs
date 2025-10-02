#![allow(dead_code)]

use clap::Parser;
use orbtc::cmd;
use sentry_tracing::layer as sentry_layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// path to config file
    #[arg(short, long, default_value_t = String::from("config.toml"))]
    config: String,

    #[arg(long, default_value_t = false)]
    runes: bool,

    #[arg(long)]
    block: Option<u64>,

    #[arg(long, default_value_t = false)]
    ignore_inputs: bool,

    #[arg(long, default_value_t = false)]
    dry_run: bool,

    #[arg(long)]
    use_firehose: bool,
}

fn init_log(sentry_enabled: bool) {
    if !sentry_enabled {
        env_logger::init();
        return;
    }

    let mut log_builder = env_logger::Builder::from_default_env();
    let logger = sentry::integrations::log::SentryLogger::with_dest(log_builder.build());
    log::set_boxed_logger(Box::new(logger)).unwrap();
    log::set_max_level(log::LevelFilter::Debug);
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let sentry_dsn = std::env::var("SENTRY_DSN");

    init_log(sentry_dsn.is_ok());
    if let Ok(dsn) = sentry_dsn {
        let environment = std::env::var("SENTRY_ENV").unwrap_or("unknown".to_string());
        let tracing_frequency: f32 = std::env::var("SENTRY_TRACES_RATES")
            .unwrap_or_default()
            .parse::<f32>()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);

        // this will send traces to sentry
        if let Err(err) = tracing_subscriber::registry()
            .with(sentry_layer())
            .try_init()
        {
            log::error!("Failed to init sentry tracing layer: {err}");
        }

        let _guard = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                environment: Some(environment.into()),
                traces_sample_rate: tracing_frequency,
                session_mode: sentry::SessionMode::Request,
                auto_session_tracking: true,
                ..Default::default()
            },
        ));
        // don't remove this.
        // This prevents Drop invocations for _guard.
        // If _guard is dropped, no events will be sent.
        std::mem::forget(_guard);
    }

    let res = if !args.runes {
        let icmd = cmd::indexer::BtcIndexer {
            dry_run: args.dry_run,
            runes: false,
            block: args.block,
            tx: None,
            use_firehose: args.use_firehose,
        };
        icmd.run(&args.config).await
    } else {
        let icmd = cmd::indexer::RuneIndexer {
            dry_run: args.dry_run,
            block: args.block,
            ignore_inputs: args.ignore_inputs,
            retry_on_fail: true,
            use_firehose: args.use_firehose,
        };
        icmd.run(&args.config).await
    };

    if let Err(err) = res.as_ref() {
        log::error!("Exit with error: {:#}", err)
    }

    res
}
