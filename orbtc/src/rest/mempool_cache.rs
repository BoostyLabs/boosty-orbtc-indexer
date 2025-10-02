use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use bitcoin::{OutPoint, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use orbtc_indexer_api::{BtcUtxo, RuneUtxo};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::config;

struct State {
    txs: HashSet<Txid>,
    utxos: HashSet<OutPoint>,
    utxos_by_tx: HashMap<Txid, Vec<OutPoint>>,
    // TODO: track
    //   txid -> vec<address>
    //   address -> (txid, in::bool, out:bool)
}

impl State {
    pub fn new() -> Self {
        Self {
            txs: HashSet::new(),
            utxos: HashSet::new(),
            utxos_by_tx: HashMap::new(),
        }
    }

    pub fn used_in_mempool(&self, out: &OutPoint) -> bool {
        self.utxos.contains(out)
    }
}

pub struct MempoolCacheManager {
    rpc: Client,
    inner: RwLock<State>,
}

impl MempoolCacheManager {
    pub fn new(btc_cfg: &config::BTCConfig) -> anyhow::Result<Self> {
        let rpc = Client::new(
            &btc_cfg.address,
            Auth::UserPass(btc_cfg.rpc_user.clone(), btc_cfg.rpc_password.clone()),
        )?;

        Ok(Self {
            rpc,
            inner: RwLock::new(State::new()),
        })
    }

    pub async fn filter_used_utxos(&self, utxos: &[BtcUtxo]) -> Vec<BtcUtxo> {
        let mi = self.inner.read().await;
        utxos
            .iter()
            .filter(|r| {
                let out = r.out_point();
                !mi.used_in_mempool(&out)
            })
            .cloned()
            .collect()
    }

    pub async fn filter_used_runes_utxos(&self, utxos: &[RuneUtxo]) -> Vec<RuneUtxo> {
        let mi = self.inner.read().await;
        utxos
            .iter()
            .filter(|r| {
                let out = r.out_point();
                !mi.used_in_mempool(&out)
            })
            .cloned()
            .collect()
    }

    async fn refresh(&self) {
        let cache = self;

        let tx_list = match cache.rpc.get_raw_mempool() {
            Ok(txs) => txs,
            Err(err) => {
                error!("can't get raw mempool: error={err}");
                return;
            }
        };

        let txs: HashSet<Txid> = tx_list.iter().cloned().collect();
        let (disappeared, appeared) = {
            let mi = cache.inner.read().await;

            let disappeared: HashSet<_> = mi.txs.difference(&txs).cloned().collect();
            let appeared: HashSet<_> = txs.difference(&mi.txs).cloned().collect();
            (disappeared, appeared)
        };

        info!(
            "Updating cache: disappeared={} appeared={}",
            disappeared.len(),
            appeared.len()
        );
        let mut new_utxos = Vec::new();
        for id in appeared.iter() {
            let tx = match cache.rpc.get_raw_transaction(id, None) {
                Ok(tx) => tx,
                Err(_) => {
                    // tx was dropped, replaced or mined
                    continue;
                }
            };

            for input in tx.input.iter() {
                new_utxos.push((*id, input.previous_output));
            }
        }

        info!("Transactions were collected");
        {
            let mut mi = cache.inner.write().await;

            for id in disappeared.iter() {
                if let Some(outs) = mi.utxos_by_tx.get(id).cloned() {
                    for o in outs.iter() {
                        mi.utxos.remove(o);
                    }
                }
                mi.utxos_by_tx.remove(id);
                mi.txs.remove(id);
            }

            for (id, out) in new_utxos.iter() {
                mi.utxos_by_tx.insert(*id, Vec::new());
                mi.utxos.insert(*out);
                mi.utxos_by_tx.entry(*id).or_default().push(*out);
            }

            mi.txs.extend(appeared);
        }

        info!("Cache updated");
    }
}

static mut MEMPOOL_UPDATE_INTERVAL: Duration = Duration::from_secs(5);
/// This method is intended for use only within integration tests.
pub fn set_mempool_update_interval(nt: Duration) {
    unsafe {
        MEMPOOL_UPDATE_INTERVAL = nt;
    }
}

pub async fn refresh_state_routine(cache: Arc<MempoolCacheManager>, cancel: CancellationToken) {
    use tokio::time::sleep;

    loop {
        info!("Refresh cache of utxos in the mempool");
        cache.refresh().await;

        tokio::select! {
            _ = unsafe { sleep(MEMPOOL_UPDATE_INTERVAL) } => {
                continue;
           }

            _ = cancel.cancelled() => {
                log::info!("refresh mempool cache task cancelled");
                break;
            }
        };
    }
}
