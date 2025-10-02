use std::str::FromStr;

use bitcoin::{Network, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::Config;
use crate::indexer::InscriptionsCacher;
use crate::{db, indexer};

#[derive(Debug, clap::Parser)]
pub struct BtcIndexer {
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub runes: bool,

    #[arg(long)]
    pub block: Option<u64>,

    #[arg(long)]
    pub tx: Option<String>,

    #[arg(long)]
    pub use_firehose: bool,
}

impl BtcIndexer {
    pub async fn dummy(&self, cfg_path: &str) -> anyhow::Result<()> {
        let cfg = Config::read(cfg_path)?;

        // create db and apply migrations if there is any
        db::apply_migrations(&cfg.db).await?;

        let cancel = CancellationToken::new();
        let tasker = TaskTracker::new();

        log::info!("Starting dummy indexer");
        let opts = indexer::IndexingOpts {
            indexer_type: indexer::IndexerType::Dummy,
            dry_run: self.dry_run,
            starting_height: self.block.unwrap_or_default(),
            skip_inputs: false,
            retry_on_fail: true,
            ord_address: None,
            use_firehose: self.use_firehose,
            firehose_api_key: cfg.firehose_api_key.clone(),
        };
        let indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
        indexer.start(&tasker, cancel.clone());

        crate::signal::ctrl_c().await;
        cancel.cancel();

        log::info!("Halting indexers");
        tasker.close();
        tasker.wait().await;

        log::info!("Application successfully shut down");
        Ok(())
    }

    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        if let Some(tx) = &self.tx {
            return self.check_tx(cfg_path, tx).await;
        }

        let cfg = Config::read(cfg_path)?;
        let starting_height = self.block.unwrap_or_default();

        // create db and apply migrations if there is any
        db::apply_migrations(&cfg.db).await?;

        let cancel = CancellationToken::new();
        let tasker = TaskTracker::new();

        log::info!("Starting bitcoin indexer");
        let opts = indexer::IndexingOpts {
            indexer_type: indexer::IndexerType::BitcoinUtxo,
            dry_run: self.dry_run,
            starting_height,
            skip_inputs: false,
            retry_on_fail: true,
            ord_address: None,
            use_firehose: self.use_firehose,
            firehose_api_key: cfg.firehose_api_key.clone(),
        };
        let btc_indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
        btc_indexer.start(&tasker, cancel.clone());

        if self.runes {
            log::info!("Starting runes indexer");
            let opts = indexer::IndexingOpts {
                indexer_type: indexer::IndexerType::Runes,
                dry_run: self.dry_run,
                starting_height,
                skip_inputs: true,
                retry_on_fail: true,
                ord_address: None,
                use_firehose: self.use_firehose,
                firehose_api_key: cfg.firehose_api_key.clone(),
            };
            let runes_indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
            runes_indexer.start(&tasker, cancel.clone());
        }

        crate::signal::ctrl_c().await;
        cancel.cancel();

        log::info!("Halting indexers");
        tasker.close();
        tasker.wait().await;

        log::info!("Application successfully shut down");
        Ok(())
    }

    pub async fn check_tx(&self, cfg_path: &str, tx_hash: &str) -> anyhow::Result<()> {
        let cfg = Config::read(cfg_path)?;

        let rpc = Client::new(
            &cfg.btc.address,
            Auth::UserPass(cfg.btc.rpc_user.clone(), cfg.btc.rpc_password.clone()),
        )
        .unwrap();

        let txid = Txid::from_str(tx_hash)?;
        let tx_info = rpc.get_raw_transaction_info(&txid, None)?;

        let block_hash = tx_info.blockhash.unwrap();
        let header_info = rpc.get_block_header_info(&block_hash)?;
        let block = rpc.get_block(&block_hash)?;

        let (txn, tx) = block
            .txdata
            .into_iter()
            .enumerate()
            .find(|(_, tx)| tx.compute_txid().eq(&txid))
            .unwrap();

        println!("tx_id = '{tx_hash}'");
        println!("height = {}", header_info.height);
        println!("blocktime = {}", tx_info.blocktime.unwrap_or_default());
        println!("txn = {}", txn);
        println!("raw_tx = '{}'", hex::encode(&tx_info.hex));

        println!("inputs: {} outputs: {}", tx.input.len(), tx.output.len());

        for input in tx.input {
            let parent_tx = input.previous_output.txid;
            let vout = input.previous_output.vout;

            println!("txid: {parent_tx} vount: {vout}");
        }

        Ok(())
    }
}

#[derive(Debug, clap::Parser)]
pub struct RuneIndexer {
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub ignore_inputs: bool,

    #[arg(long, default_value_t = false)]
    pub retry_on_fail: bool,

    #[arg(long)]
    pub block: Option<u64>,

    #[arg(long)]
    pub use_firehose: bool,
}

impl RuneIndexer {
    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        let cfg = Config::read(cfg_path)?;

        if cfg.db.automigrate {
            // create db and apply migrations if there is any
            db::apply_migrations(&cfg.db).await?;
        }
        let cancel = CancellationToken::new();
        let tasker = TaskTracker::new();
        log::info!("Starting runes indexer");
        let starting_height = if cfg.btc.get_network() == Network::Bitcoin {
            self.block.unwrap_or(840_000 - 6)
        } else {
            self.block.unwrap_or_default()
        };

        let opts = indexer::IndexingOpts {
            indexer_type: indexer::IndexerType::Runes,
            dry_run: self.dry_run,
            starting_height,
            skip_inputs: self.ignore_inputs,
            retry_on_fail: self.retry_on_fail,
            ord_address: None,
            use_firehose: self.use_firehose,
            firehose_api_key: cfg.firehose_api_key.clone(),
        };
        let runes_indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
        runes_indexer.start(&tasker, cancel.clone());
        tasker.close();

        crate::signal::ctrl_c().await;
        cancel.cancel();

        log::info!("Halting runes indexer");
        tasker.wait().await;
        log::info!("Application successfully shut down");
        Ok(())
    }
}

#[derive(Debug, clap::Parser)]
pub struct InscriptionsIndexer {
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    #[arg(long, default_value_t = false)]
    pub retry_on_fail: bool,

    #[arg(long, default_value_t = false)]
    pub load_dump: bool,

    #[arg(long)]
    pub dump_path: Option<String>,

    #[arg(long, default_value_t = 0)]
    pub from: u64,
}

impl InscriptionsIndexer {
    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        let cfg = Config::read(cfg_path)?;

        if cfg.db.automigrate {
            // create db and apply migrations if there is any
            db::apply_migrations(&cfg.db).await?;
        }

        let cancel = CancellationToken::new();
        let tasker = TaskTracker::new();
        log::info!("Starting inscriptions indexer");
        if self.load_dump {
            let path = self.dump_path.clone().unwrap();
            let ctx = cancel.clone();
            tasker.spawn(async move {
                let mut w = InscriptionsCacher::new(&cfg.db);
                return w.quick_import(ctx, &path, 0).await;
            });
            tasker.close();

            crate::signal::ctrl_c().await;
            cancel.cancel();

            log::info!("Halting inscriptions indexer");
            tasker.wait().await;
            log::info!("Application successfully shut down");

            return Ok(());
        }
        let mut starting_height = if cfg.btc.get_network() == Network::Bitcoin {
            767430 - 1
        } else {
            anyhow::bail!(
                "{} is not supported. This indexer works only with mainnet",
                cfg.btc.get_network()
            );
        };

        if self.from > starting_height {
            starting_height = self.from;
        }

        let opts = indexer::IndexingOpts {
            indexer_type: indexer::IndexerType::InscriptionsCache,
            dry_run: self.dry_run,
            starting_height,
            skip_inputs: true,
            retry_on_fail: self.retry_on_fail,
            ord_address: cfg.ord_api.address.clone(),
            use_firehose: false,
            firehose_api_key: None,
        };
        let runes_indexer = indexer::BlockIndexerRt::new(&cfg.db, &cfg.btc, opts);
        runes_indexer.start(&tasker, cancel.clone());
        tasker.close();

        crate::signal::ctrl_c().await;
        cancel.cancel();

        log::info!("Halting inscriptions indexer");
        tasker.wait().await;
        log::info!("Application successfully shut down");
        Ok(())
    }
}
