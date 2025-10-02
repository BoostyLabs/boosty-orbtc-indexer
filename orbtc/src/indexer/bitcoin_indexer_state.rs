use std::collections::HashSet;

use diesel::Connection;

use super::db::*;
use crate::db::schema::{Address as AddressRow, Input, Output};

pub struct StateProvider {
    pub db: DB,
    pub dataset: BlockData,
    pub address_index: HashSet<String>,
}

impl StateProvider {
    pub fn new(db: DB) -> Self {
        Self {
            db,
            dataset: BlockData {
                new_addresses: Vec::with_capacity(16_000),
                new_inputs: Vec::with_capacity(16_000),
                new_outputs: Vec::with_capacity(16_000),
            },
            address_index: HashSet::with_capacity(10_000_000),
        }
    }

    pub fn commit_state(&mut self) -> anyhow::Result<()> {
        info!(
            "Commiting indexer state: new_outputs={} new_inputs={}",
            self.dataset.new_outputs.len(),
            self.dataset.new_inputs.len(),
        );

        let conn = &mut self.db.conn;
        conn.transaction(|conn| {
            if let Err(err) = DB::insert_addresses(conn, &self.dataset.new_addresses) {
                error!(
                    "can't insert new addresses: len={} err={}",
                    self.dataset.new_addresses.len(),
                    err
                );
                return diesel::result::QueryResult::Err(err);
            }

            if let Err(err) = DB::insert_outputs(conn, &self.dataset.new_outputs) {
                error!(
                    "can't insert new outputs: len={} err={}",
                    self.dataset.new_outputs.len(),
                    err
                );
                return diesel::result::QueryResult::Err(err);
            }

            if let Err(err) = DB::insert_inputs(conn, &self.dataset.new_inputs) {
                error!(
                    "can't insert new inputs: len={} err={}",
                    self.dataset.new_inputs.len(),
                    err
                );
                return diesel::result::QueryResult::Err(err);
            }

            diesel::result::QueryResult::Ok(())
        })?;

        self.reset_state();
        Ok(())
    }

    pub fn reset_state(&mut self) {
        self.dataset.new_addresses.clear();
        self.dataset.new_outputs.clear();
        self.dataset.new_inputs.clear();

        if self.address_index.len() > 10_000_000 {
            self.address_index.clear();
        }
    }
}

#[derive(Default)]
pub struct BlockData {
    pub new_addresses: Vec<AddressRow>,
    pub new_inputs: Vec<Input>,
    pub new_outputs: Vec<Output>,
}
