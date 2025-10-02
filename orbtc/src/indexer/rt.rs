use std::thread::sleep;

use bitcoin::{BlockHash, Transaction, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::bitcoin_indexer::BitcoinUtxoIndexer;
use super::db;
use super::inscriptions_index::InscriptionsCacheIndexer;
use super::runes_indexer::RunesIndexer;
use crate::config;
use crate::db::schema;

pub trait TxIndexer {
    fn name(&self) -> String;
    fn index_transaction(&mut self, tx_info: &TxInfo) -> anyhow::Result<()>;
    fn commit_state(&mut self) -> anyhow::Result<()>;
    fn reset_state(&mut self);
}

#[derive(Default, Debug, Clone, Copy)]
pub enum IndexerType {
    #[default]
    Dummy,
    BitcoinUtxo,
    Runes,
    InscriptionsCache,
}

#[derive(Default, Debug, Clone)]
pub struct IndexingOpts {
    pub indexer_type: IndexerType,
    pub dry_run: bool,
    pub starting_height: u64,
    pub retry_on_fail: bool,
    pub skip_inputs: bool,
    pub ord_address: Option<String>,
    pub use_firehose: bool,
    pub firehose_api_key: Option<String>,
}

pub struct TxInfo<'a> {
    pub block: u64,
    pub tx_n: i32,
    // todo use Txid type here
    pub txid: Txid,
    pub tx: &'a Transaction,
    pub timestamp: i64,
}

pub struct BlockIndexerRt {
    db_cfg: config::DBConfig,
    btc_cfg: config::BTCConfig,
    opts: IndexingOpts,
}

impl BlockIndexerRt {
    pub fn new(db_cfg: &config::DBConfig, btc_cfg: &config::BTCConfig, opts: IndexingOpts) -> Self {
        Self {
            db_cfg: db_cfg.clone(),
            btc_cfg: btc_cfg.clone(),
            opts,
        }
    }

    pub fn start(self, tasker: &TaskTracker, cancel: CancellationToken) {
        tasker.spawn_blocking(move || {
            let rt = Rt::new(&self.db_cfg, &self.btc_cfg, self.opts);
            rt.run(cancel);
            info!("indexer stopped")
        });
    }
}

struct Rt {
    name: String,
    opts: IndexingOpts,

    db: db::DB,
    rpc: Client,

    // TODO: add option to run
    // multiple indexers within one instance of RT
    indexer: Box<dyn TxIndexer>,

    last_block: Option<BlockHash>,
    #[allow(dead_code)]
    use_firehose: bool,
    #[cfg(feature = "firehose")]
    fh_client: crate::firehose::FHClient,
}

impl Rt {
    fn new(db_cfg: &config::DBConfig, btc_cfg: &config::BTCConfig, opts: IndexingOpts) -> Self {
        let net = btc_cfg.get_network();
        let rpc = Client::new(
            &btc_cfg.address,
            Auth::UserPass(btc_cfg.rpc_user.clone(), btc_cfg.rpc_password.clone()),
        )
        .unwrap();
        let db = db::DB::establish_connection(&db_cfg.dsn);

        let indexer: Box<dyn TxIndexer> = match opts.indexer_type {
            IndexerType::Dummy => Box::new(Dummy {}),
            IndexerType::BitcoinUtxo => Box::new(BitcoinUtxoIndexer::new(net, db_cfg)),
            IndexerType::Runes => Box::new(RunesIndexer::new(db_cfg, btc_cfg, opts.skip_inputs)),
            IndexerType::InscriptionsCache => Box::new(InscriptionsCacheIndexer::new(
                db_cfg,
                opts.ord_address
                    .clone()
                    .expect("ord address isn't set")
                    .as_str(),
            )),
        };

        let use_firehose = opts.use_firehose;

        #[cfg(feature = "firehose")]
        let fh_client = firehose::FHClient::new(opts.firehose_api_key.unwrap());

        Self {
            db,
            rpc,
            opts,
            last_block: None,
            name: indexer.name(),
            indexer,
            use_firehose,
            #[cfg(feature = "firehose")]
            fh_client,
        }
    }

    fn run(self, cancel: CancellationToken) {
        let mut indexer = self;

        while !cancel.is_cancelled() {
            if !indexer._run(&cancel) && indexer.opts.retry_on_fail {
                error!("Run failed. Retry");
                unsafe {
                    sleep(super::INDEXER_WAIT_INTERVAL);
                }
                indexer.indexer.reset_state();
                continue;
            }
            break;
        }
    }

    fn _run(&mut self, cancel: &CancellationToken) -> bool {
        let first_block = self.starting_block();

        let mut best_block = match self.rpc.get_block_count() {
            Ok(count) => count,
            Err(err) => {
                error!("Can't get best BTC block error={:#?}", err);
                error!("Indexing stopped");
                return false;
            }
        };

        info!(
            "RPC init successful! best_block={} first_block={}",
            best_block, first_block
        );

        let mut current_block = first_block;
        while !cancel.is_cancelled() {
            best_block = match self.rpc.get_block_count() {
                Ok(count) => count,
                Err(err) => {
                    error!("Can't get best BTC block error={:#?}", err);
                    return false;
                }
            };

            if best_block < current_block {
                unsafe {
                    sleep(super::INDEXER_WAIT_INTERVAL);
                }
                continue;
            }

            debug!(
                "BTC INDEXER: BTC best block={}, last indexed={}",
                best_block, current_block
            );

            let (height, hash, tx_count) = match self.index_block(current_block) {
                Ok(v) => v,
                Err(err) => {
                    error!("Block indexing failed. Retry.: error={err}");
                    continue;
                }
            };

            if height < current_block || tx_count == 0 {
                info!(
                    "Fork occured. Reseting state to fork root: height={} hash={}",
                    current_block, hash,
                );
                if !self.opts.dry_run {
                    if let Err(err) = self.db.update_last_block(&self.name, height as i64) {
                        error!("Unable to update last indexed block: error={:#?}", err);
                        return false;
                    }
                }
                self.last_block = Some(hash);
                current_block = height + 1;
                continue;
            }

            info!(
                "Processed new block: height={} hash={} tx_count={}",
                current_block, hash, tx_count
            );

            if !self.opts.dry_run {
                if let Err(err) = self.db.update_last_block(&self.name, current_block as i64) {
                    error!("Unable to update last indexed block: error={:#?}", err);
                }
            }

            self.last_block = Some(hash);

            current_block += 1;
        }

        info!("Received stop signal. Indexing stopped");
        true
    }

    fn starting_block(&mut self) -> u64 {
        let result = self.db.get_last_indexed_block(&self.name);
        let last_block = match result {
            #[rustfmt::skip] // starting from next after saved
            Ok(b) => if b > 0 { (b + 1) as u64 } else { 0 },
            Err(_) => {
                if let Err(err) = self.db.update_last_block(&self.name, 0) {
                    error!("Failed to insert indexer tip: err={err}");
                }

                0
            }
        };
        last_block.max(self.opts.starting_height)
    }

    pub fn find_fork_root(&mut self, block_hash: BlockHash) -> anyhow::Result<schema::Block> {
        let mut block_hash = block_hash;
        let indexer = self.indexer.name();
        loop {
            if let Ok(b) = self.db.get_block(&block_hash.into(), &indexer) {
                return Ok(b);
            }

            let header = self.rpc.get_block_header_info(&block_hash)?;
            let Some(prev_hash) = header.previous_block_hash else {
                anyhow::bail!("block({block_hash}) has no parent");
            };

            block_hash = prev_hash;
        }
    }
    fn fetch_block(&mut self, height: u64) -> anyhow::Result<(BlockHash, bitcoin::Block)> {
        #[cfg(feature = "firehose")]
        if self.use_firehose {
            let block = self.fh_client.get_block(height)?;
            return Ok(block);
        }

        let block_hash = match self.rpc.get_block_hash(height) {
            Ok(hash) => hash,
            Err(err) => {
                anyhow::bail!("Can't get BTC block hash: height={height} error={:#?}", err,);
            }
        };

        let block: bitcoin::Block = match self.rpc.get_by_id(&block_hash) {
            Ok(block) => block,
            Err(err) => {
                anyhow::bail!(
                    "Can't get BTC block by hash for heigh({height}): hash={block_hash} error={:#?}",
                    err
                );
            }
        };

        Ok((block_hash, block))
    }

    fn index_block(&mut self, height: u64) -> anyhow::Result<(u64, BlockHash, usize)> {
        let (block_hash, block) = self.fetch_block(height)?;

        debug!(
            "Fetch new block: height={} hash={} tx_count={}",
            height,
            block_hash,
            block.txdata.len()
        );

        if self
            .last_block
            .map(|b| b.ne(&block.header.prev_blockhash))
            .unwrap_or(false)
        {
            match self.find_fork_root(block.header.prev_blockhash) {
                Ok(root) => {
                    if !self.opts.skip_inputs {
                        if let Err(err) = self.db.drop_blocks(root.height + 1, &self.indexer.name())
                        {
                            anyhow::bail!("[BUG]: can't drop orphans: error={:#?}", err);
                        }
                    } else {
                        // TODO: will be improved and clarifyied in next release
                        if let Err(err) = self
                            .db
                            .drop_runes_blocks(root.height + 1, &self.indexer.name())
                        {
                            anyhow::bail!("[BUG]: can't drop orphans: error={:#?}", err);
                        }
                    }
                    // TODO: insert orphaned block
                    let hash = BlockHash::from(&root.hash);
                    return Ok((root.height as u64, hash, 0));
                }

                Err(err) => {
                    anyhow::bail!("unable to find fork root: error={err}");
                }
            }
        }

        for (txi, tx) in block.txdata.iter().enumerate() {
            let tx_info = TxInfo {
                block: height,
                tx_n: txi as i32,
                txid: tx.compute_txid(),
                tx,
                timestamp: block.header.time as i64,
            };

            if let Err(err) = self.indexer.index_transaction(&tx_info) {
                error!(
                    "[BUG]: can't proceed without data corruption: error={:#?}",
                    err
                );
                anyhow::bail!("[BUG]: can't proceed without data corruption");
            }
        }

        if self.opts.dry_run {
            return Ok((height, block_hash, block.txdata.len()));
        }

        if let Err(err) = self.indexer.commit_state() {
            error!(
                "[BUG] Can't commit block data error={:#}, hash={}",
                err, block_hash
            );

            anyhow::bail!("[BUG] Can't commit block data",);
        }

        let res = self.db.insert_block(
            height as i64,
            &block_hash.into(),
            block.header.time as i64,
            &self.indexer.name(),
        );
        if let Err(err) = res {
            error!(
                "[BUG] Can't insert block tip: hash={block_hash}, error={:#}",
                err
            );
            anyhow::bail!("[BUG] Can't insert block tip: hash={block_hash}",);
        }

        Ok((height, block_hash, block.txdata.len()))
    }
}

pub struct Dummy {}

impl TxIndexer for Dummy {
    fn name(&self) -> String {
        "dummy-indexer".into()
    }

    fn index_transaction(&mut self, tx_info: &TxInfo) -> anyhow::Result<()> {
        debug!(
            "->> block={} tx_n={} tx={} inputs={} outputs={}",
            tx_info.block,
            tx_info.tx_n,
            tx_info.txid,
            tx_info.tx.input.len(),
            tx_info.tx.output.len()
        );

        Ok(())
    }

    fn commit_state(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset_state(&mut self) {}
}
