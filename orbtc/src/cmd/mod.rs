use anyhow::Context;
use api_core::server::{run_metrics_server, run_server};
use clap::Parser;
use prometheus::Registry;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::Config;
use crate::db;
use crate::rest::api::Service;
use crate::rest::metrics;

pub mod api_keys;
pub mod indexer;
pub mod migrate;
use indexer::{BtcIndexer, InscriptionsIndexer, RuneIndexer};

#[derive(Debug, Parser)]
pub enum Subcommand {
    #[command(about = "Start API server only")]
    ApiServer,

    #[command(
        about = "Run indexer. By default indexes only bitcoin utxos, to index Runes, set --runes flag"
    )]
    Indexer(BtcIndexer),

    #[command(about = "Run runes-only indexer")]
    RuneIndexer(RuneIndexer),

    #[command(about = "Run indexer that marks utxo with inscriptions")]
    InscriptionsIndexer(InscriptionsIndexer),

    #[command(about = "Run dummy indexer; only for tests")]
    Dummy(BtcIndexer),

    #[command(about = "Prints default config structure")]
    ExampleConfig,

    #[command(about = "Helper to decode and pretty print Tx or PSBT")]
    ExtractTx(ExtractTxCmd),

    #[command(subcommand, about = "Manage API Keys")]
    ApiKey(api_keys::ManageApiKeys),

    #[command(subcommand, about = "Manage Indexer DB")]
    Db(migrate::DbCmd),
}

impl Subcommand {
    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        match self {
            Subcommand::ApiServer => run_api_server(cfg_path).await,
            Subcommand::Dummy(cmd) => cmd.dummy(cfg_path).await,
            Subcommand::Indexer(cmd) => cmd.run(cfg_path).await,
            Subcommand::RuneIndexer(cmd) => cmd.run(cfg_path).await,
            Subcommand::InscriptionsIndexer(cmd) => cmd.run(cfg_path).await,
            Subcommand::ExtractTx(cmd) => cmd.run(),
            Subcommand::ApiKey(cmd) => cmd.run(cfg_path).await,
            Subcommand::Db(cmd) => cmd.run(cfg_path).await,
            Subcommand::ExampleConfig => {
                let cfg = Config::default();
                let output = toml::to_string_pretty(&cfg)?;
                println!("{output}");
                Ok(())
            }
        }
    }
}

#[derive(Debug, Parser)]
pub struct ExtractTxCmd {
    #[arg(long)]
    tx: Option<String>,
    #[arg(long)]
    psbt: Option<String>,
}

impl ExtractTxCmd {
    fn run(&self) -> anyhow::Result<()> {
        use base64::Engine;

        if let Some(tx) = self.tx.clone() {
            let data = hex::decode(&tx)?;

            let tx: bitcoin::Transaction = bitcoin::consensus::deserialize(&data)?;

            dbg!(&tx);

            for i in tx.input {
                println!(
                    "-> inputs: tx_hash: {} vout: {}",
                    i.previous_output.txid, i.previous_output.vout
                );
            }
        }

        if let Some(psbt) = self.psbt.clone() {
            let raw_psbt = base64::prelude::BASE64_STANDARD.decode(&psbt)?;
            let psbt = bitcoin::psbt::Psbt::deserialize(&raw_psbt)?;

            dbg!(&psbt);
        }
        Ok(())
    }
}

pub async fn run_api_server(cfg_path: &str) -> anyhow::Result<()> {
    let cfg = Config::read(cfg_path).context("unable to read config file")?;
    if cfg.db.automigrate {
        db::apply_migrations(&cfg.db).await?;
    }

    log::info!("Init api service");
    let api_service = Service::new(cfg.clone()).await?;
    let tasker = TaskTracker::new();
    let cancel = CancellationToken::new();
    log::info!("Spawn api jobs");
    api_service.spawn_jobs(cancel.clone());

    log::info!("Run HTTP server");
    let mregistry = if cfg.metrics.enable {
        let registry = metrics::registry();

        tasker.spawn_local(run_metrics_server(
            cfg.metrics.clone(),
            cancel.clone(),
            registry.clone(),
        ));
        Some(registry)
    } else {
        None
    };

    run_api(cfg, cancel.clone(), api_service, mregistry.clone()).await;
    tasker.close();
    cancel.cancel();

    log::info!("Halting indexer API");
    tasker.wait().await;
    log::info!("Application successfully shut down");

    Ok(())
}

async fn run_api(
    cfg: Config,
    cancel: CancellationToken,
    api_service: Service,
    registry: Option<Registry>,
) {
    match run_server(cfg.api, cancel, api_service, registry).await {
        Ok(_) => (),
        Err(err) => {
            error!("HTTP server failed: {:?}", err);
        }
    }

    log::info!("Indexer API stopped");
}

pub async fn run_all_indexer_services(
    cfg: Config,
    tasker: tokio_util::task::TaskTracker,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    use crate::indexer;
    let _ = env_logger::try_init();

    // create db and apply migrations if there is any
    db::apply_migrations(&cfg.db).await?;

    log::info!("Starting bitcoin indexer");
    let opts = indexer::IndexingOpts {
        indexer_type: indexer::IndexerType::BitcoinUtxo,
        dry_run: false,
        starting_height: 0,
        skip_inputs: false,
        retry_on_fail: true,
        ord_address: None,
        use_firehose: false,
        firehose_api_key: None,
    };

    let btc_indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
    btc_indexer.start(&tasker, cancel.clone());

    log::info!("Starting runes indexer");
    let opts = indexer::IndexingOpts {
        indexer_type: indexer::IndexerType::Runes,
        dry_run: false,
        starting_height: 0,
        skip_inputs: true,
        retry_on_fail: true,
        ord_address: None,
        use_firehose: false,
        firehose_api_key: None,
    };

    let runes_indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
    runes_indexer.start(&tasker, cancel.clone());

    let api_service = Service::new(cfg.clone()).await?;
    api_service.spawn_jobs(cancel.clone());
    let api_future = run_server(cfg.api, cancel.clone(), api_service, None);

    match api_future.await {
        Ok(_) => (),
        Err(err) => {
            error!("HTTP server failed: {:?}", err);
            anyhow::bail!("HTTP server failed: {:?}", err);
        }
    }

    log::info!("Application successfully shut down");

    Ok(())
}
