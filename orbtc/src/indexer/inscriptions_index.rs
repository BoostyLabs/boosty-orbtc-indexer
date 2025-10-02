use std::collections::BTreeMap;
use std::str::FromStr;

use diesel::Connection;
use tokio_util::sync::CancellationToken;

use super::db;
use super::db::*;
use super::rt::{TxIndexer, TxInfo};
use crate::db::schema::OutputExtras;
use crate::{config, ord_api};

// do not change this value. If you do, modify migration!
pub const INSCRIPTIONS_CACHE_INDEX: &str = "inscriptions_cache_index";

struct State {
    db: db::DB,
    dataset: Vec<OutputExtras>,
}

pub struct InscriptionsCacher {
    state: State,
}

impl InscriptionsCacher {
    pub fn new(db_cfg: &config::DBConfig) -> Self {
        let db = db::DB::establish_connection(&db_cfg.dsn);

        Self {
            state: State {
                db,
                dataset: Vec::new(),
            },
        }
    }
}

impl InscriptionsCacher {
    pub async fn quick_import(
        &mut self,
        cancel: CancellationToken,
        path: &str,
        wid: usize,
    ) -> anyhow::Result<()> {
        use std::io::{BufRead, BufReader};
        let file = std::fs::File::open(path)?;

        let reader = BufReader::new(file);
        for (id, line) in reader.lines().enumerate() {
            if cancel.is_cancelled() {
                info!("#[{wid}] task cancelled. finish");
                return Ok(());
            }
            let line = line?;
            let mut parts = line.split(",");

            let Some(txid) = parts.next() else {
                continue;
            };
            let Some(vout) = parts.next() else {
                continue;
            };
            let vout: i32 = vout.parse()?;
            let hash = orbtc_indexer_api::Hash::from_str(txid).expect("valid hash");

            info!("#[{wid}] [{id}] loading...");
            let utxo_id = match self.state.db.select_output_id(&hash, vout) {
                Ok(id) => id,
                Err(err) => {
                    error!(
                        "#[{wid}] can't fetch utxo id: n={id} hash={} vout={} error={err:#?}",
                        hash, vout
                    );
                    continue;
                }
            };

            let utxo = OutputExtras {
                id: utxo_id,
                has_runes: false,
                has_inscriptions: true,
            };

            self.state.dataset.push(utxo);
            if self.state.dataset.len() >= 2000 {
                if let Err(err) = self.commit_state(wid) {
                    error!("#[{wid}] can't commit state: n={id} error={err:#?}");
                    continue;
                }
            }
        }

        Ok(())
    }
    fn commit_state(&mut self, wid: usize) -> anyhow::Result<()> {
        info!(
            "#[{wid}] Committing indexer state: outputs={}",
            self.state.dataset.len(),
        );
        let conn = &mut self.state.db.conn;
        conn.transaction(|conn| DB::insert_utxo_extras(conn, &self.state.dataset))?;

        self.state.dataset.clear();

        Ok(())
    }
}

pub struct InscriptionsCacheIndexer {
    state: State,
    // address: String,
    ord_client: ord_api::OrdClientSync,
}

impl InscriptionsCacheIndexer {
    pub fn new(db_cfg: &config::DBConfig, ord_address: &str) -> Self {
        let db = db::DB::establish_connection(&db_cfg.dsn);

        let ord_client = ord_api::OrdClientSync::new(ord_address);
        Self {
            state: State {
                db,
                dataset: Vec::new(),
            },
            // address: ord_address.to_owned(),
            ord_client,
        }
    }
}

impl TxIndexer for InscriptionsCacheIndexer {
    fn name(&self) -> String {
        INSCRIPTIONS_CACHE_INDEX.into()
    }

    fn index_transaction(&mut self, tx_info: &TxInfo) -> anyhow::Result<()> {
        let hash = tx_info.txid.into();
        let vout_ids = self.state.db.select_output_ids(&hash)?;
        let hash_str = hash.to_hex_string();
        debug!(
            "Index transaction: [{}/{}] tx={} outputs={}",
            tx_info.block,
            tx_info.tx_n,
            hash_str,
            vout_ids.len()
        );

        let mut idx = BTreeMap::new();
        let mut request = Vec::with_capacity(vout_ids.len());
        for (id, vout) in vout_ids {
            let outpoint = format!("{}:{}", hash_str, vout);
            idx.insert(outpoint, id);
            request.push(format!("{}:{}", hash_str, vout));
        }
        let res = match self.ord_client.get_details(&request) {
            Ok(v) => v,
            Err(err) => {
                // this is optional index, it's ok to skip on error.
                warn!(
                    "can't get details from ord for tx outputs: tx={} err={err:#?}",
                    hash_str
                );
                return Ok(());
            }
        };

        for i in res {
            let has_inscriptions = !i.inscriptions.is_empty();
            let has_runes = !i.runes.is_empty();
            if !has_inscriptions && !has_runes {
                continue;
            }
            let Some(id) = idx.get(&i.outpoint) else {
                continue;
            };
            let utxo = OutputExtras {
                id: *id,
                has_runes,
                has_inscriptions,
            };
            self.state.dataset.push(utxo);
        }

        Ok(())
    }

    fn commit_state(&mut self) -> anyhow::Result<()> {
        info!(
            "Committing indexer state: outputs={}",
            self.state.dataset.len(),
        );
        let conn = &mut self.state.db.conn;
        conn.transaction(|conn| DB::insert_utxo_extras(conn, &self.state.dataset))?;

        self.state.dataset.clear();

        Ok(())
    }

    fn reset_state(&mut self) {
        self.state.dataset.clear();
    }
}
