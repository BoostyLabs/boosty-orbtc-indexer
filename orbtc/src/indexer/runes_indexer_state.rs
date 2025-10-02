use std::collections::{BTreeSet, HashMap, HashSet};

use diesel::Connection;
use ordinals::RuneId;

use super::db::*;
use crate::db::schema::*;

pub struct State {
    pub db: DB,
    dataset: BlockData,
    pub address_index: HashSet<String>,
}

impl State {
    pub fn new(db: DB) -> Self {
        Self {
            db,
            dataset: BlockData {
                runes_index: HashMap::with_capacity(10_000),
                new_runes: HashMap::with_capacity(10_000),
                rune_updates: HashMap::with_capacity(10_000),
                new_utxos: Vec::with_capacity(16_000),
                new_inputs: Vec::with_capacity(16_000),
                new_addresses: Vec::with_capacity(10_000),
            },
            address_index: HashSet::with_capacity(10_000_000),
        }
    }

    pub fn get_rune_by_name(&mut self, rune: &str) -> anyhow::Result<Rune> {
        if let Some(r) = self.dataset.new_runes.get(rune) {
            return Ok(r.clone());
        }

        if let Some(r) = self.dataset.rune_updates.get(rune) {
            return Ok(r.clone());
        }

        let rune_row = self.db.get_rune(rune)?;
        self.dataset
            .runes_index
            .insert(rune_row.rune_id(), rune.to_string());
        self.dataset
            .rune_updates
            .insert(rune.to_owned(), rune_row.clone());
        Ok(rune_row)
    }

    pub fn get_rune_by_id(&mut self, rune_id: &ordinals::RuneId) -> anyhow::Result<Rune> {
        if let Some(name) = self.dataset.runes_index.get(rune_id) {
            if let Some(r) = self.dataset.new_runes.get(name) {
                return Ok(r.clone());
            }

            if let Some(r) = self.dataset.rune_updates.get(name) {
                return Ok(r.clone());
            }
        }

        let rune_row = self
            .db
            .get_rune_by_id(rune_id.block as i64, rune_id.tx as i32)?;

        self.dataset
            .runes_index
            .insert(rune_row.rune_id(), rune_row.name.clone());
        self.dataset
            .rune_updates
            .insert(rune_row.name.clone(), rune_row.clone());
        Ok(rune_row)
    }

    pub fn get_rune_name_by_id(&mut self, rune_id: &ordinals::RuneId) -> Option<String> {
        if let Ok(rune) = self.get_rune_by_id(rune_id) {
            return Some(rune.name);
        }
        None
    }

    pub fn store_new_rune(&mut self, rune_row: &Rune) -> anyhow::Result<()> {
        self.dataset
            .new_runes
            .insert(rune_row.name.clone(), rune_row.clone());

        self.dataset
            .runes_index
            .insert(rune_row.rune_id(), rune_row.name.clone());
        Ok(())
    }

    pub fn burn_rune_by_id(&mut self, id: &RuneId, amount: u128) -> anyhow::Result<()> {
        let mut rune_info = self.get_rune_by_id(id)?;
        rune_info.burn(amount);

        self.update_rune(rune_info);
        Ok(())
    }

    pub fn update_rune(&mut self, rune: Rune) {
        let rune_id = rune.name.clone();

        if let std::collections::hash_map::Entry::Occupied(mut e) =
            self.dataset.new_runes.entry(rune_id.clone())
        {
            e.insert(rune);
            return;
        }

        self.dataset.rune_updates.insert(rune_id, rune);
    }

    pub fn update_rune_mint(&mut self, rune: &Rune) {
        self.update_rune(rune.to_owned());
    }

    pub fn add_address(&mut self, address_row: Address) {
        self.address_index.insert(address_row.address.clone());
        self.dataset.new_addresses.push(address_row);
    }

    pub fn store_new_runes_utxo(&mut self, utxo: RuneUtxo) {
        self.dataset.new_utxos.push(utxo);
    }

    pub fn get_parent_utxos(&mut self, input: &bitcoin::TxIn) -> Option<BTreeSet<RuneUtxo>> {
        use orbtc_indexer_api::types::Hash;
        let parent_txid: Hash = input.previous_output.txid.into();
        let vout = input.previous_output.vout;

        let Ok(utxos) = self.db.select_runes_outputs(&parent_txid, vout as i32) else {
            error!("can't get utxo from db");
            return None;
        };

        let mut res_list: BTreeSet<RuneUtxo> = utxos.iter().cloned().collect::<BTreeSet<_>>();
        if utxos.is_empty() {
            for u in self.dataset.new_utxos.iter_mut() {
                if u.tx_hash.eq(&parent_txid) && u.vout == vout as i32 {
                    res_list.insert(u.clone());
                }
            }

            if !res_list.is_empty() {
                return Some(res_list);
            }
            return None;
        }

        Some(res_list)
    }

    pub fn commit_state(&mut self, skip_inputs: bool) -> anyhow::Result<()> {
        info!(
            "Commiting indexer state: new_runes={} upd_runes={} new_utxos={}",
            self.dataset.new_runes.len(),
            self.dataset.rune_updates.len(),
            self.dataset.new_utxos.len(),
        );

        let conn = &mut self.db.conn;
        conn.transaction(|conn| {
            let runes: Vec<Rune> = self.dataset.new_runes.values().cloned().collect();

            if let Err(err) = DB::insert_runes(conn, &runes) {
                error!("can't insert new runes: len={} err={}", runes.len(), err);
                return diesel::result::QueryResult::Err(err);
            }

            for v in self.dataset.rune_updates.values() {
                DB::update_rune(
                    conn,
                    &v.name,
                    v.mints,
                    &v.minted,
                    &v.burned,
                    &v.in_circulation,
                )?;
            }
            if let Err(err) = DB::insert_addresses(conn, &self.dataset.new_addresses) {
                error!(
                    "can't insert new addresses: len={} err={}",
                    self.dataset.new_addresses.len(),
                    err
                );
                return diesel::result::QueryResult::Err(err);
            }

            let mut unique = HashSet::new();
            let mut rows: Vec<RuneUtxo> = Vec::new();
            for u in self.dataset.new_utxos.iter() {
                let k = (&u.tx_hash, u.vout, &u.rune);

                if !unique.contains(&k) {
                    unique.insert(k);
                    rows.push(u.clone());
                    continue;
                }

                if u.amount.0 == 0 {
                    continue;
                }
            }

            if let Err(err) = DB::insert_rune_utxos(conn, &rows) {
                error!(
                    "can't insert new utxos: len={} err={}",
                    self.dataset.new_utxos.len(),
                    err
                );
                return diesel::result::QueryResult::Err(err);
            }
            if !skip_inputs {
                if let Err(err) = DB::insert_inputs(conn, &self.dataset.new_inputs) {
                    error!(
                        "can't insert new inputs: len={} err={}",
                        self.dataset.new_inputs.len(),
                        err
                    );
                    return diesel::result::QueryResult::Err(err);
                }
            }

            diesel::result::QueryResult::Ok(())
        })?;

        self.reset_state();
        Ok(())
    }

    pub fn reset_state(&mut self) {
        self.dataset.new_runes.clear();
        self.dataset.rune_updates.clear();
        self.dataset.new_utxos.clear();
        self.dataset.new_inputs.clear();
        self.dataset.new_addresses.clear();

        if self.address_index.len() > 10_000_000 {
            self.address_index.clear();
        }
    }
}

#[derive(Default)]
pub struct BlockData {
    runes_index: HashMap<RuneId, String>,
    new_runes: HashMap<String, Rune>,
    rune_updates: HashMap<String, Rune>,
    new_utxos: Vec<RuneUtxo>,
    new_inputs: Vec<Input>,
    new_addresses: Vec<Address>,
}
