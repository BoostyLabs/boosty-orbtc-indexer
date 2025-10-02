use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use bitcoincore_rpc::json::EstimateMode;
use bitcoincore_rpc::{Auth, RpcApi};
use instant::{Duration, Instant};
use orbtc_indexer_api::{BtcUtxo, RuneUtxo, StatusResponse};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use super::mempool_cache::MempoolCacheManager;
use super::requests::FeeRate;
use crate::config::Config;
use crate::db::{open_postgres_db, Repo};
use crate::mempool_api::MempoolClient;
use crate::rest::metrics;
use crate::{cache, db};

#[derive(Clone)]
pub struct Context {
    pub net: bitcoin::Network,
    pub cfg: crate::config::Config,

    pub db: Arc<Repo>,
    pub cache: Arc<Option<cache::Repo>>,
    pub btc_client: Arc<bitcoincore_rpc::Client>,

    pub metrics_collector: Arc<MetricsCollector>,
    pub mempool_index: Arc<MempoolCacheManager>,
    pub cached_fee: Arc<RwLock<Option<(FeeRate, Instant)>>>,

    pub api_keys: HashMap<String, db::ApiKey>,
}

impl Context {
    pub async fn new(cfg: Config) -> anyhow::Result<Self> {
        let repo: Repo = open_postgres_db(&cfg.db).await?;
        let db = Arc::new(repo);

        let net = cfg.btc.get_network();
        let auth = Auth::UserPass(cfg.btc.rpc_user.clone(), cfg.btc.rpc_password.clone());
        let btc = bitcoincore_rpc::Client::new(&cfg.btc.address, auth)?;

        let btc_client = Arc::new(btc);
        let mi = MempoolCacheManager::new(&cfg.btc)?;
        let metrics_collector = MetricsCollector::new(db.clone(), btc_client.clone());
        let cache_repo = if cfg.cache.enable {
            Some(cache::Repo::new(&cfg.cache.redis, cfg.cache.lock_ttl).await?)
        } else {
            None
        };

        // Right now new api key can be added only manually.
        // So, we load all keys once at start.
        let api_keys = db
            .select_api_keys()
            .await?
            .iter()
            .map(|e| (e.key.clone(), e.clone()))
            .collect();

        Ok(Self {
            db,
            btc_client,
            net,
            cfg,
            cache: Arc::new(cache_repo),
            cached_fee: Arc::new(RwLock::new(None)),
            metrics_collector: Arc::new(metrics_collector),
            mempool_index: Arc::new(mi),
            api_keys,
        })
    }

    pub async fn estimate_fee(&self) -> anyhow::Result<FeeRate> {
        const CACHE_TTL: Duration = Duration::from_secs(10);

        // Read from cache.
        if let Some((cached, instant)) = *self.cached_fee.read().await {
            if Instant::now().duration_since(instant) < CACHE_TTL {
                return Ok(cached);
            }
        }

        let fee = MempoolClient::new(self.net, self.cfg.fee_adjustement)
            .get_fee()
            .await;

        let fee = match fee {
            Ok(fee) => fee,
            Err(err) => {
                info!("Unable to fetch fee from mempool. Fallback to node rpc: error={err}");
                let fee = match self.estimate_fee_local().await {
                    Ok(fee) => fee,
                    Err(err) => {
                        if self.net == bitcoin::Network::Regtest {
                            FeeRate {
                                fast: 3,
                                normal: 2,
                                min: 1,
                            }
                        } else {
                            return Err(err);
                        }
                    }
                };
                // Do not cache fee by two reasons:
                // 1. Fee rates from mempool api is more relevant for us;
                // 2. Node RPC has no request limits.
                return Ok(fee);
            }
        };

        // Successful response, update the cache.
        *self.cached_fee.write().await = Some((fee, Instant::now()));

        Ok(fee)
    }

    pub async fn estimate_fee_local(&self) -> anyhow::Result<FeeRate> {
        const CACHE_TTL: Duration = Duration::from_secs(10);

        // Read from cache.
        if let Some((cached, instant)) = *self.cached_fee.read().await {
            if Instant::now().duration_since(instant) < CACHE_TTL {
                return Ok(cached);
            }
        }

        // Fetch fees from a local node.
        let fastest_fee = get_fee_local(&self.btc_client, 1, EstimateMode::Conservative).await?;
        let normal_fee = get_fee_local(&self.btc_client, 3, EstimateMode::Conservative).await?;
        let min_fee = get_fee_local(&self.btc_client, 6, EstimateMode::Economical).await?;

        let fee = FeeRate {
            fast: fastest_fee,
            normal: normal_fee,
            min: min_fee,
        };

        // Successful response, update the cache.
        *self.cached_fee.write().await = Some((fee, Instant::now()));

        Ok(fee)
    }

    pub async fn filter_runes_utxos(&self, utxo: &[BtcUtxo]) -> anyhow::Result<Vec<BtcUtxo>> {
        let txs: Vec<_> = utxo.iter().map(|u| &u.tx_hash).collect();
        let runes_outs = self.db.select_runes_utxo_for_txs(&txs).await?;
        let runes_outs: BTreeSet<_> = runes_outs.iter().map(|o| (&o.tx_hash, o.vout)).collect();

        let result: Vec<_> = utxo
            .iter()
            .filter(|u| !runes_outs.contains(&(&u.tx_hash, u.vout)))
            .cloned()
            .collect();

        Ok(result)
    }

    pub async fn filter_used_btc_utxos(
        &self,
        utxos: &[BtcUtxo],
        check_no_runes: bool,
        request_id: Option<String>,
    ) -> anyhow::Result<Vec<BtcUtxo>> {
        let mut rows = self.mempool_index.filter_used_utxos(utxos).await;

        let utxo_ids: Vec<_> = utxos.iter().map(|u| u.id).collect();
        let has_inscriptions: BTreeSet<_> = self
            .db
            .select_outputs_extras(&utxo_ids)
            .await?
            .iter()
            .filter_map(|e| if e.has_inscriptions { Some(e.id) } else { None })
            .collect();

        let runes_outs = if check_no_runes {
            let txs: Vec<_> = utxos.iter().map(|u| &u.tx_hash).collect();
            let runes_outs = self.db.select_runes_utxo_for_txs(&txs).await?;
            let runes_outs: BTreeSet<_> = runes_outs
                .iter()
                .map(|o| (o.tx_hash.clone(), o.vout))
                .collect();
            runes_outs
        } else {
            BTreeSet::new()
        };

        let mut filtered = Vec::new();
        for r in rows {
            // TODO: make cache request optional
            let locked = if let Some(repo) = self.cache.as_ref() {
                repo.check_is_locked(&r.tx_hash, r.vout, &request_id)
                    .await?
            } else {
                false
            };

            if locked
                || runes_outs.contains(&(r.tx_hash.clone(), r.vout))
                || has_inscriptions.contains(&r.id)
            {
                continue;
            }

            filtered.push(r)
        }
        rows = filtered;

        Ok(rows)
    }

    pub async fn filter_used_runes_utxos(
        &self,
        utxos: &[RuneUtxo],
        request_id: Option<String>,
    ) -> anyhow::Result<Vec<RuneUtxo>> {
        let utxo_ids: Vec<_> = utxos.iter().map(|u| u.id).collect();
        let has_inscriptions: BTreeSet<_> = self
            .db
            .select_outputs_extras_by_rune_ids(&utxo_ids)
            .await?
            .iter()
            .filter_map(|e| if e.has_inscriptions { Some(e.id) } else { None })
            .collect();
        let mut rows = self.mempool_index.filter_used_runes_utxos(utxos).await;
        let mut filtered = Vec::new();
        for r in rows {
            // TODO: make cache request optional
            let locked = if let Some(repo) = self.cache.as_ref() {
                repo.check_is_locked(&r.tx_hash, r.vout, &request_id)
                    .await?
            } else {
                false
            };

            if locked || has_inscriptions.contains(&r.id) {
                continue;
            }

            filtered.push(r)
        }
        rows = filtered;

        Ok(rows)
    }

    pub async fn is_healthy(&self) -> bool {
        self.metrics_collector.service_status().await.healthy
    }

    pub fn get_api_key(&self, api_key: &str) -> Option<db::ApiKey> {
        self.api_keys.get(api_key).cloned()
    }
}

async fn get_fee_local(
    rpc: &bitcoincore_rpc::Client,
    blocks: u16,
    mode: EstimateMode,
) -> anyhow::Result<u64> {
    use anyhow::Context;
    let resp = rpc
        .estimate_smart_fee(blocks, Some(mode))
        .context(format!("can't get fee for {} blocks", blocks))?;

    if let Some(errors) = resp.errors {
        anyhow::bail!("local BTC node returned errors: {:?}", errors);
    }

    // this is a BTC/kvB fee rate. we need to convert it to sat/vB
    let feerate = resp
        .fee_rate
        .ok_or_else(|| anyhow::anyhow!("local BTC node returned no fee rate"))?;

    // this is a sat per kilo vbyte rate. divide by 1000 to get sat/vbyte.
    let fee_sat_per_vbyte = feerate.to_sat() / 1000;
    Ok(fee_sat_per_vbyte)
}

pub struct MetricsCollector {
    db: Arc<Repo>,
    btc_client: Arc<bitcoincore_rpc::Client>,
    status: Arc<RwLock<Option<(StatusResponse, Instant)>>>,
}

impl MetricsCollector {
    pub fn new(db: Arc<Repo>, btc_client: Arc<bitcoincore_rpc::Client>) -> Self {
        Self {
            db,
            btc_client,
            status: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn service_status(&self) -> StatusResponse {
        #[cfg(test)]
        const CACHE_TTL: Duration = Duration::from_millis(200);
        #[cfg(not(test))]
        const CACHE_TTL: Duration = Duration::from_secs(10);

        // Read from cache.
        if let Some((cached, instant)) = *self.status.read().await {
            if Instant::now().duration_since(instant) < CACHE_TTL {
                return cached;
            }
        }

        let status = self.aggregate_status().await;
        *self.status.write().await = Some((status, Instant::now()));
        status
    }

    pub async fn aggregate_status(&self) -> StatusResponse {
        use crate::indexer::BITCOIN_INDEX;
        let (btc_node, btc_height) = match self.btc_client.get_block_count() {
            Ok(val) => (true, val),
            Err(err) => {
                error!("failed to get BTC block count: error={:#?}", err);
                (false, 0)
            }
        };

        let mut db = true;
        let btc = match self.db.get_last_indexed_block(BITCOIN_INDEX).await {
            Ok(block) => block,
            Err(err) => {
                db = false;
                error!("failed to get bitcoin indexer status: error={:#?}", err);
                0
            }
        };

        let mut runes = 0;
        use crate::indexer::RUNES_INDEX;
        match self.db.get_last_indexed_block(RUNES_INDEX).await {
            Ok(block) => runes = block,
            Err(err) => {
                db = false;
                error!("failed to get bitcoin indexer status: error={:#?}", err);
            }
        }

        let btc_indexer_ok = btc_height.max(btc) - btc <= 3;
        let runes_indexer_ok = btc_height.max(runes) - runes <= 3;
        let healthy = db && btc_node && btc_indexer_ok && runes_indexer_ok;

        if !healthy {
            error!(
                "Indexer API is unhealthy: db={} btc={} height={} btc_indexer={} runes_indexer={}",
                db, btc_node, btc_height, btc, runes,
            );
        }

        StatusResponse {
            healthy,
            db,
            btc_node,
            btc_height,
            btc_indexer: btc_indexer_ok,
            btc_indexer_height: btc,
            runes_indexer: runes_indexer_ok,
            runes_indexer_height: runes,
        }
    }
}

pub async fn update_metrics(cache: Arc<MetricsCollector>, cancel: CancellationToken) {
    use std::time::Duration;

    use tokio::time::sleep;

    loop {
        info!("Update metrics status");
        let status = cache.service_status().await;
        metrics::update(status);

        tokio::select! {
            _ = sleep(Duration::from_secs(10)) => {
                continue;
           }

            _ = cancel.cancelled() => {
                log::info!("updated metrics task cancelled");
                break;
            }
        };
    }
}
