use diesel;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use orbtc_indexer_api::types::{Amount, Hash};

use crate::db::schema::{tables, *};

pub struct DB {
    pub conn: PgConnection,
}

impl DB {
    pub fn establish_connection(database_url: &str) -> Self {
        let conn = PgConnection::establish(database_url)
            .unwrap_or_else(|_| panic!("Error connecting to {}", database_url));
        Self { conn }
    }

    pub fn insert_addresses(conn: &mut PgConnection, rows: &Vec<Address>) -> QueryResult<()> {
        use tables::addresses::dsl::*;
        if rows.is_empty() {
            return Ok(());
        }
        // There is a limit how many values we can insert in one query.
        // > driver error: "number of parameters must be between 0 and 65535"
        if rows.len() > 3000 {
            for r in rows.chunks(3000) {
                diesel::insert_into(addresses)
                    .values(r)
                    .on_conflict(address)
                    .do_nothing()
                    .execute(conn)?;
            }
            return Ok(());
        }

        diesel::insert_into(addresses)
            .values(rows)
            .on_conflict(address)
            .do_nothing()
            .execute(conn)?;

        Ok(())
    }

    pub fn insert_outputs(conn: &mut PgConnection, rows: &Vec<Output>) -> QueryResult<()> {
        use tables::outputs::dsl::*;
        if rows.is_empty() {
            return Ok(());
        }
        // There is a limit how many values we can insert in one query.
        // > driver error: "number of parameters must be between 0 and 65535"
        if rows.len() > 3000 {
            for r in rows.chunks(3000) {
                diesel::insert_into(outputs).values(r).execute(conn)?;
            }
            return Ok(());
        }

        diesel::insert_into(outputs).values(rows).execute(conn)?;

        Ok(())
    }

    pub fn select_output_ids(&mut self, tx_hash_v: &Hash) -> QueryResult<Vec<(i64, i32)>> {
        use tables::outputs::dsl::*;
        outputs
            .filter(tx_hash.eq(tx_hash_v))
            .select((id, vout))
            .load::<(i64, i32)>(&mut self.conn)
    }

    pub fn select_output_id(&mut self, tx_hash_v: &Hash, output_n: i32) -> QueryResult<i64> {
        use tables::outputs::dsl::*;
        outputs
            .filter(tx_hash.eq(tx_hash_v))
            .filter(vout.eq(output_n))
            .select(id)
            .first::<i64>(&mut self.conn)
    }

    pub fn insert_inputs(conn: &mut PgConnection, rows: &Vec<Input>) -> QueryResult<()> {
        use tables::inputs::dsl::*;
        if rows.is_empty() {
            return Ok(());
        }
        // There is a limit how many values we can insert in one query.
        // > driver error: "number of parameters must be between 0 and 65535"
        if rows.len() > 3000 {
            for r in rows.chunks(3000) {
                diesel::insert_into(inputs).values(r).execute(conn)?;
            }
            return Ok(());
        }

        diesel::insert_into(inputs).values(rows).execute(conn)?;

        Ok(())
    }

    pub fn get_last_indexed_block(&mut self, name: &str) -> anyhow::Result<i64> {
        use tables::last_indexed_block::dsl::*;

        let row: LastIndexedBlock = last_indexed_block
            .filter(indexer.eq(name))
            .first(&mut self.conn)?;

        Ok(row.height)
    }

    pub fn update_last_block(&mut self, name: &str, height_v: i64) -> anyhow::Result<()> {
        use tables::last_indexed_block::dsl::*;

        // this implements an UPSERT (insert if doesn't exist, update if exists).
        diesel::insert_into(last_indexed_block)
            .values((indexer.eq(name), height.eq(height_v)))
            .on_conflict(indexer)
            .do_update()
            .set(height.eq(height_v))
            .execute(&mut self.conn)?;

        Ok(())
    }

    pub fn insert_block(
        &mut self,
        height: i64,
        hash: &Hash,
        time: i64,
        indexer: &str,
    ) -> anyhow::Result<()> {
        use tables::blocks::dsl;
        diesel::insert_into(dsl::blocks)
            .values((
                dsl::height.eq(height),
                dsl::hash.eq(hash),
                dsl::blocktime.eq(time),
                dsl::indexer.eq(indexer),
            ))
            .execute(&mut self.conn)?;

        Ok(())
    }

    pub fn get_block(&mut self, hash: &Hash, indexer_name: &str) -> anyhow::Result<Block> {
        use tables::blocks::dsl as blocks_dsl;
        let row: Block = blocks_dsl::blocks
            .filter(blocks_dsl::hash.eq(hash))
            .filter(blocks_dsl::indexer.eq(indexer_name))
            .first(&mut self.conn)?;

        Ok(row)
    }

    pub fn drop_blocks(&mut self, height: i64, indexer: &str) -> anyhow::Result<()> {
        let conn = &mut self.conn;
        conn.transaction(|conn| {
            use tables::blocks::dsl as blocks_dsl;
            diesel::delete(blocks_dsl::blocks)
                .filter(blocks_dsl::height.ge(height))
                .filter(blocks_dsl::indexer.eq(indexer))
                .execute(conn)?;

            use tables::inputs::dsl as inputs_dsl;
            diesel::delete(inputs_dsl::inputs)
                .filter(inputs_dsl::block.ge(height))
                .execute(conn)?;

            use tables::outputs::dsl as outputs_dsl;
            diesel::delete(outputs_dsl::outputs)
                .filter(outputs_dsl::block.ge(height))
                .execute(conn)?;

            use tables::runes_outputs::dsl as runes_outs_dsl;
            diesel::delete(runes_outs_dsl::runes_outputs)
                .filter(runes_outs_dsl::block.ge(height))
                .execute(conn)?;

            use tables::runes::dsl as runes_dsl;
            diesel::delete(runes_dsl::runes)
                .filter(runes_dsl::block.ge(height))
                .execute(conn)?;

            diesel::result::QueryResult::Ok(())
        })?;

        Ok(())
    }

    pub fn drop_runes_blocks(&mut self, height: i64, indexer: &str) -> anyhow::Result<()> {
        let conn = &mut self.conn;
        conn.transaction(|conn| {
            use tables::blocks::dsl as blocks_dsl;
            diesel::delete(blocks_dsl::blocks)
                .filter(blocks_dsl::height.ge(height))
                .filter(blocks_dsl::indexer.eq(indexer))
                .execute(conn)?;

            use tables::runes_outputs::dsl as runes_outs_dsl;
            diesel::delete(runes_outs_dsl::runes_outputs)
                .filter(runes_outs_dsl::block.ge(height))
                .execute(conn)?;

            use tables::runes::dsl as runes_dsl;
            diesel::delete(runes_dsl::runes)
                .filter(runes_dsl::block.ge(height))
                .execute(conn)?;

            diesel::result::QueryResult::Ok(())
        })?;

        Ok(())
    }

    pub fn insert_runes(conn: &mut PgConnection, rune_rows: &Vec<Rune>) -> QueryResult<()> {
        use tables::runes::dsl::*;
        if rune_rows.is_empty() {
            return Ok(());
        }

        diesel::insert_into(runes).values(rune_rows).execute(conn)?;
        Ok(())
    }

    pub fn update_rune(
        conn: &mut PgConnection,
        rune_name: &str,
        mints_v: i32,
        minted_v: &Amount,
        burned_v: &Amount,
        in_circulation_v: &Amount,
    ) -> QueryResult<()> {
        use tables::runes::dsl::*;

        diesel::update(runes)
            .filter(name.eq(rune_name))
            .set((
                mints.eq(mints_v),
                minted.eq(minted_v),
                burned.eq(burned_v),
                in_circulation.eq(in_circulation_v),
            ))
            .execute(conn)?;

        Ok(())
    }

    pub fn get_rune(&mut self, rune_name: &str) -> anyhow::Result<Rune> {
        use tables::runes::dsl::*;

        let rune_rows = runes
            .filter(name.eq(rune_name))
            .select(Rune::as_select())
            .first(&mut self.conn)?;

        Ok(rune_rows)
    }

    pub fn get_rune_by_id(&mut self, q_block: i64, q_tx: i32) -> anyhow::Result<Rune> {
        use tables::runes::dsl::*;

        let rune_rows = runes
            .filter(block.eq(q_block))
            .filter(tx_id.eq(q_tx))
            .select(Rune::as_select())
            .first(&mut self.conn)?;
        Ok(rune_rows)
    }

    pub fn insert_rune_utxos(conn: &mut PgConnection, rows: &Vec<RuneUtxo>) -> QueryResult<()> {
        use tables::runes_outputs::dsl::*;
        if rows.is_empty() {
            return Ok(());
        }
        // There is a limit how many values we can insert in one query.
        // > driver error: "number of parameters must be between 0 and 65535"
        if rows.len() > 3000 {
            for r in rows.chunks(3000) {
                diesel::insert_into(runes_outputs).values(r).execute(conn)?;
            }
            return Ok(());
        }

        diesel::insert_into(runes_outputs)
            .values(rows)
            .execute(conn)?;

        Ok(())
    }

    pub fn select_runes_outputs(
        &mut self,
        tx_hash_v: &Hash,
        vout_v: i32,
    ) -> anyhow::Result<Vec<RuneUtxo>> {
        use tables::runes_outputs::dsl::*;

        let rows: Vec<RuneUtxo> = runes_outputs
            .filter(tx_hash.eq(tx_hash_v))
            .filter(vout.eq(vout_v))
            .select(RuneUtxo::as_select())
            .load(&mut self.conn)?;
        Ok(rows)
    }

    pub fn get_address(&mut self, address: &str) -> anyhow::Result<Address> {
        use tables::addresses::dsl;

        let row: Address = dsl::addresses
            .filter(dsl::address.eq(address))
            .select(Address::as_select())
            .first(&mut self.conn)?;
        Ok(row)
    }

    pub fn insert_utxo_extras(
        conn: &mut PgConnection,
        rows: &Vec<OutputExtras>,
    ) -> QueryResult<()> {
        use tables::outputs_extras::dsl::*;
        if rows.is_empty() {
            return Ok(());
        }
        // There is a limit how many values we can insert in one query.
        // > driver error: "number of parameters must be between 0 and 65535"
        if rows.len() > 3000 {
            for r in rows.chunks(3000) {
                diesel::insert_into(outputs_extras)
                    .values(r)
                    .on_conflict_do_nothing()
                    .execute(conn)?;
            }
            return Ok(());
        }

        diesel::insert_into(outputs_extras)
            .values(rows)
            .on_conflict_do_nothing()
            .execute(conn)?;

        Ok(())
    }
}
