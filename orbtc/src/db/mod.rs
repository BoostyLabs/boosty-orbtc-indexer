#![allow(clippy::too_many_arguments)]

use bigdecimal::BigDecimal;
use orbtc_indexer_api::types::Hash;
use orbtc_indexer_api::*;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::{PgPoolOptions, PgQueryResult};
use sqlx::prelude::FromRow;
use sqlx::{PgPool, Postgres, QueryBuilder, Result};

use crate::config::DBConfig;

pub mod models;
pub mod query_builder;
pub mod schema;
pub mod seed_data;

pub use models::*;
use query_builder::DynamicQueryBuilder;
use seed_data::*;

static MIGRATOR: Migrator = sqlx::migrate!("src/db/migrations");

pub async fn open_postgres_db(config: &DBConfig) -> Result<Repo> {
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .connect(&config.dsn)
        .await?;
    let repo = Repo { pool };

    Ok(repo)
}

pub fn get_migration_info() -> Vec<(
    i64,
    std::borrow::Cow<'static, str>,
    std::borrow::Cow<'static, [u8]>,
)> {
    let mut info = Vec::new();
    for m in MIGRATOR.iter() {
        info.push((m.version, m.description.clone(), m.checksum.clone()))
    }
    info
}

pub async fn apply_migrations(config: &DBConfig) -> Result<()> {
    let pool = PgPoolOptions::new()
        .max_connections(100)
        .connect(&config.dsn)
        .await?;
    let repo = Repo { pool };

    repo.migrate(config.force_migration).await?;

    if repo.get_rune(FIRST_RUNE).await?.is_none() {
        repo.insert_seed_data().await?;
        #[cfg(feature = "test-tweak")]
        {
            let mut row = ApiKey::new("test-client");
            row.key = "TEST_API_KEY".into();
            repo.insert_api_key(row).await?;
        }
    }

    Ok(())
}

#[derive(FromRow)]
struct Count {
    count: i64,
}

#[derive(Debug)]
pub struct Repo {
    pub pool: PgPool,
}

impl Repo {
    pub async fn migrate(&self, force_migration: bool) -> Result<(), MigrateError> {
        loop {
            let Err(migrate_err) = MIGRATOR.run(&self.pool).await else {
                return Ok(());
            };

            if !force_migration {
                return Err(migrate_err);
            }
            warn!(
                "Migration failed with error, force_mode is on, trying to repair. error={:#}",
                migrate_err
            );

            match migrate_err {
                MigrateError::VersionMismatch(v) => {
                    for m in MIGRATOR.iter() {
                        if m.version < v {
                            continue;
                        }

                        sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2")
                            .bind(m.checksum.to_vec())
                            .bind(m.version)
                            .execute(&self.pool)
                            .await?;
                    }

                    // try again
                    continue;
                }
                MigrateError::VersionMissing(version) => {
                    sqlx::query("DELETE FROM _sqlx_migrations WHERE version >= $1")
                        .bind(version)
                        .execute(&self.pool)
                        .await?;
                    // try again
                    continue;
                }
                _ => (),
            }
            return Err(migrate_err);
        }
    }

    pub async fn reset_schema(&self) -> Result<()> {
        let _ = sqlx::query("DROP SCHEMA public CASCADE")
            .execute(&self.pool)
            .await?;
        let _ = sqlx::query("CREATE SCHEMA public")
            .execute(&self.pool)
            .await?;
        // db is clean, nothing to force
        self.migrate(false).await?;
        Ok(())
    }

    pub async fn insert_seed_data(&self) -> Result<()> {
        let rune_row = reserved_rune();
        self.insert_rune(&rune_row).await?;

        Ok(())
    }

    pub async fn exec_raw(&self, query: &str) -> Result<PgQueryResult> {
        let query = sqlx::query(query);
        let result = query.execute(&self.pool).await?;
        Ok(result)
    }

    pub async fn get_last_indexed_blocks(&self) -> Result<Vec<LastIndexedBlock>> {
        let result = sqlx::query_as::<_, LastIndexedBlock>("SELECT * FROM last_indexed_block")
            .fetch_all(&self.pool)
            .await?;

        Ok(result)
    }

    pub async fn get_last_indexed_block(&self, indexer: &str) -> Result<u64> {
        let result = sqlx::query_as::<_, LastIndexedBlock>(
            "SELECT * FROM last_indexed_block WHERE indexer = $1",
        )
        .bind(indexer)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|i| i.height as u64).unwrap_or_default())
    }

    pub async fn get_balance(&self, address: &str) -> Result<Balance> {
        let result = sqlx::query_as::<_, Balance>(
            r#"SELECT address, balance::BIGINT, utxo_count
               FROM balances WHERE address = $1"#,
        )
        .bind(address)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.unwrap_or(Balance {
            address: address.into(),
            ..Default::default()
        }))
    }

    pub async fn count_utxos(&self, address: &str) -> Result<i64> {
        let result = sqlx::query_as::<_, Count>(
            r#"SELECT count(1) as count
               FROM utxos WHERE address = $1"#,
        )
        .bind(address)
        .fetch_one(&self.pool)
        .await?;

        Ok(result.count)
    }

    pub async fn get_address_btc_utxo_ge_amount(
        &self,
        address: &str,
        amount: u64,
    ) -> Result<Option<BtcUtxo>> {
        let result = sqlx::query_as::<_, BtcUtxo>(
            r#"
            SELECT *
            FROM utxos
            WHERE
                address = $1 AND
                amount >= $2
            ORDER BY amount ASC
            LIMIT 1"#,
        )
        .bind(address)
        .bind(amount as i64)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn select_utxo_with_pagination(
        &self,
        address: &str,
        order: OrderBy,
        amount_threshold: Option<u64>,
        skip_premature: Option<u64>,
        sorting: UtxoSortMode,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<BtcUtxo>> {
        let mut q = QueryBuilder::new("SELECT * FROM utxos WHERE address = ");
        q.push_bind(address);
        if let Some(am) = amount_threshold {
            q.push(" AND amount > ");
            q.push_bind(am as i64);
        }

        if let Some(block) = skip_premature {
            q.push(" AND ((coinbase = true AND block < ");
            q.push_bind(block as i64);
            q.push(") OR coinbase = false) ");
        }

        match sorting {
            UtxoSortMode::Age => {
                q.push(format!(" ORDER BY block {order}, tx_id {order} "));
            }
            UtxoSortMode::Amount => {
                q.push(format!(" ORDER BY amount {order} "));
            }
        }

        q.push(" LIMIT ");
        q.push_bind(limit as i32);
        q.push(" OFFSET ");
        q.push_bind(offset as i32);

        let result = q.build_query_as::<BtcUtxo>().fetch_all(&self.pool).await?;
        Ok(result)
    }

    pub async fn select_tx_outputs(&self, tx_hash: &Hash) -> Result<Vec<BtcOutput>> {
        sqlx::query_as::<_, BtcOutput>(
            r#"SELECT
                o.id,
                o.block,
                o.tx_id,
                o.tx_hash,
                o.vout,
                a.address,
                a.pk_script,
                o.amount,
                o.coinbase,
                false as spend
               FROM outputs o
               INNER JOIN addresses a
                  ON o.address = a.address
               WHERE o.tx_hash = $1"#,
        )
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_outputs_sum(&self, address: &str) -> Result<Vec<BtcOutputsSum>> {
        sqlx::query_as::<_, BtcOutputsSum>(
            r#"SELECT
                o.block,
                o.tx_id,
                o.address,
                sum(o.amount)::BIGINT as amount,
                count(*) as count
               FROM outputs o
               WHERE o.address = $1
               GROUP BY o.block, o.tx_id, o.address
               ORDER BY o.block, o.tx_id"#,
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_inputs_ext(&self, tx_hash: &Hash) -> Result<Vec<InputFull>> {
        sqlx::query_as::<_, InputFull>(
            r#"SELECT
                i.id,
                i.block,
                i.tx_id,
                i.tx_hash,
                i.vin,
                i.parent_tx,
                i.parent_vout,
                o.block as parent_block,
                o.tx_id as parent_tx_id,
                a.address,
                a.pk_script,
                o.amount,
                o.coinbase
               FROM inputs i
               INNER JOIN outputs o
                  ON i.parent_tx = o.tx_hash AND i.parent_vout = o.vout
               INNER JOIN addresses a
                  ON o.address = a.address
               WHERE i.tx_hash = $1"#,
        )
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_inputs_sum(&self, address: &str) -> Result<Vec<InputsSum>> {
        sqlx::query_as::<_, InputsSum>(
            r#"SELECT
                i.block,
                i.tx_id,
                o.address,
                sum(o.amount)::BIGINT as amount,
                count(*) as count
               FROM inputs i
               INNER JOIN outputs o
                  ON i.parent_tx = o.tx_hash AND i.parent_vout = o.vout
               WHERE o.address = $1
               GROUP BY i.block, i.tx_id, o.address
               ORDER by i.block, i.tx_id"#,
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_utxos_with_amount_bounds(
        &self,
        address: &str,
        limit: u32,
        lower_bound: u64,
        upper_bound: u64,
        skip_premature: u64,
    ) -> Result<Vec<BtcUtxo>> {
        let result = sqlx::query_as::<_, BtcUtxo>(
            r#"
            SELECT *
            FROM utxos
            WHERE
                address = $1
                AND (amount >= $2 AND amount <= $3)
                AND ((coinbase = true AND block < $4) OR coinbase = false)
            ORDER BY amount DESC
            LIMIT $5"#,
        )
        .bind(address)
        .bind(lower_bound as i64)
        .bind(upper_bound as i64)
        .bind(skip_premature as i64)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn insert_rune(&self, rune: &Rune) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO runes (
                    rune_id,
                    name,
                    display_name,
                    symbol,
                    block,
                    tx_id,
                    mints,
                    max_supply,
                    minted,
                    in_circulation,
                    divisibility,
                    turbo,
                    block_time,
                    etching_tx,
                    commitment_tx,
                    raw_data,
                    premine,
                    burned,
                    is_featured)
                  VALUES($1, $2, $3, $4, $5, $6, $7, $8,
                         $9, $10, $11, $12, $13, $14, $15,
                         $16, $17, $18, $19)",
        )
        .bind(&rune.rune_id)
        .bind(&rune.name)
        .bind(&rune.display_name)
        .bind(&rune.symbol)
        .bind(rune.block)
        .bind(rune.tx_id)
        .bind(rune.mints)
        .bind(&rune.max_supply)
        .bind(&rune.minted)
        .bind(&rune.in_circulation)
        .bind(rune.divisibility)
        .bind(rune.turbo)
        .bind(rune.block_time)
        .bind(&rune.etching_tx)
        .bind(&rune.commitment_tx)
        .bind(&rune.raw_data)
        .bind(&rune.premine)
        .bind(&rune.burned)
        .bind(rune.is_featured)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_rune(&self, rune: &str) -> Result<Option<Rune>> {
        let result = sqlx::query_as::<_, Rune>("SELECT * FROM runes WHERE name = $1")
            .bind(rune)
            .fetch_optional(&self.pool)
            .await?;

        Ok(result)
    }

    pub async fn list_runes(
        &self,
        order: OrderBy,
        limit: u32,
        offset: u32,
        name: Option<String>,
        is_featured: Option<bool>,
    ) -> Result<Vec<Rune>> {
        let mut q: DynamicQueryBuilder<Postgres> = DynamicQueryBuilder::new("SELECT * FROM runes");

        q.add_and("is_featured = ", is_featured)
            .add_and("name ILIKE ", name.map(|n| format!("%{}%", n)));
        let q = q.query();

        q.push(format!(" ORDER BY block {order}, tx_id {order} "));
        q.push(" LIMIT ");
        q.push_bind(limit as i32);
        q.push(" OFFSET ");
        q.push_bind(offset as i32);

        let query = q.build_query_as::<Rune>();
        let results = query.fetch_all(&self.pool).await?;
        Ok(results)
    }

    pub async fn count_runes(
        &self,
        name: Option<String>,
        is_featured: Option<bool>,
    ) -> Result<i64> {
        let mut q: DynamicQueryBuilder<Postgres> =
            DynamicQueryBuilder::new("SELECT count(1) as count FROM runes");

        q.add_and("is_featured = ", is_featured)
            .add_and("name ILIKE ", name.map(|n| format!("%{}%", n)));

        let q = q.query();
        let query = q.build_query_as::<Count>();
        let result = query.fetch_one(&self.pool).await?;

        Ok(result.count)
    }

    pub async fn search_runes(&self, pattern: &str) -> Result<Vec<Rune>> {
        let q = "SELECT * FROM runes WHERE name ILIKE $1 ORDER BY block ASC, tx_id ASC LIMIT 50";
        let p = format!("{}%", pattern);
        let result = sqlx::query_as::<_, Rune>(q)
            .bind(&p)
            .fetch_all(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn count_runes_utxo(&self, rune: &str, address: &str) -> Result<i64> {
        let mut q = QueryBuilder::new("SELECT count(1) as count FROM runes_utxos WHERE rune = ");
        q.push_bind(rune);
        q.push(" AND address = ");
        q.push_bind(address);

        let result = q.build_query_as::<Count>().fetch_one(&self.pool).await?;
        Ok(result.count)
    }

    pub async fn select_rune_utxo_with_pagination(
        &self,
        rune: &str,
        address: &str,
        order: OrderBy,
        amount_threshold: Option<u64>,
        sorting: UtxoSortMode,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RuneUtxo>> {
        let mut q = QueryBuilder::new("SELECT * FROM runes_utxos WHERE address = ");
        q.push_bind(address);
        q.push(" AND rune = ");
        q.push_bind(rune);
        if let Some(am) = amount_threshold {
            q.push(" AND amount > ");
            q.push_bind(am as i64);
        }

        match sorting {
            UtxoSortMode::Age => {
                q.push(format!(" ORDER BY block {order}, tx_id {order} "));
            }
            UtxoSortMode::Amount => {
                q.push(format!(" ORDER BY amount {order} "));
            }
        }

        q.push(" LIMIT ");
        q.push_bind(limit as i32);
        q.push(" OFFSET ");
        q.push_bind(offset as i32);

        let result = q.build_query_as::<RuneUtxo>().fetch_all(&self.pool).await?;
        Ok(result)
    }

    pub async fn select_rune_utxos_with_amount_bounds(
        &self,
        address: &str,
        rune: &str,
        limit: u32,
        lower_bound: &BigDecimal,
        upper_bound: &BigDecimal,
    ) -> Result<Vec<RuneUtxo>> {
        let result = sqlx::query_as::<_, RuneUtxo>(
            r#"
            SELECT *
            FROM runes_utxos
            WHERE
                address = $1 AND rune = $2 AND
                (amount >= $3 AND amount <= $4)
            ORDER BY amount DESC
            LIMIT $5"#,
        )
        .bind(address)
        .bind(rune)
        .bind(lower_bound)
        .bind(upper_bound)
        .bind(limit as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn get_address_rune_utxo_ge_amount(
        &self,
        address: &str,
        rune: &str,
        amount: u128,
    ) -> Result<Option<RuneUtxo>> {
        let result = sqlx::query_as::<_, RuneUtxo>(
            r#"
            SELECT *
            FROM runes_utxos
            WHERE
                address = $1 AND
                rune = $2 AND
                amount >= $3
            ORDER BY amount ASC
            LIMIT 1"#,
        )
        .bind(address)
        .bind(rune)
        .bind(amount as i64)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn get_runes_balances(&self, address: &str) -> Result<Vec<RuneBalance>> {
        let result = sqlx::query_as::<_, RuneBalance>(
            r#"
            SELECT
                b.address,
                b.rune,
                b.rune_id,
                r.symbol,
                r.divisibility,
                b.balance,
                b.btc_balance::bigint,
                b.utxo_count
            FROM
                runes_balances b
            JOIN
                runes r ON b.rune = r.name
            WHERE
                b.address = $1
            "#,
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await?;
        Ok(result)
    }

    pub async fn get_rune_balance(&self, address: &str, rune: &str) -> Result<RuneBalance> {
        let result = sqlx::query_as::<_, RuneBalance>(
            r#"
            SELECT
                b.address,
                b.rune,
                b.rune_id,
                r.symbol,
                r.divisibility,
                b.balance,
                b.btc_balance::bigint,
                b.utxo_count
            FROM
                runes_balances b
            JOIN
                runes r ON b.rune = r.name
            WHERE
                b.address = $1 AND b.rune = $2
            "#,
        )
        .bind(address)
        .bind(rune)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.unwrap_or(RuneBalance {
            address: address.into(),
            rune: rune.into(),
            ..Default::default()
        }))
    }

    pub async fn get_rune_holders(
        &self,
        rune: &str,
        order: OrderBy,
        amount_threshold: Option<u64>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RuneBalance>> {
        let mut q = QueryBuilder::new(
            r#"
            SELECT
                b.address,
                b.rune,
                b.rune_id,
                r.symbol,
                r.divisibility,
                b.balance,
                b.btc_balance::bigint,
                b.utxo_count
            FROM
                runes_balances b
            JOIN runes r ON b.rune = r.name
            WHERE b.rune = "#,
        );
        q.push_bind(rune);

        if let Some(am) = amount_threshold {
            q.push(" AND b.balance > ");
            q.push_bind(am as i64);
        }

        q.push(format!(" ORDER BY b.balance {order} "));
        q.push(" LIMIT ");
        q.push_bind(limit as i32);
        q.push(" OFFSET ");
        q.push_bind(offset as i32);

        let result = q
            .build_query_as::<RuneBalance>()
            .fetch_all(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn select_runes_utxo_for_txs(&self, tx_ids: &[&Hash]) -> Result<Vec<ShortTxOut>> {
        sqlx::query_as::<_, ShortTxOut>(
            r#"SELECT tx_hash, vout FROM runes_utxos
            WHERE tx_hash = ANY($1)"#,
        )
        .bind(tx_ids)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_outputs_extras(&self, ids: &[i64]) -> Result<Vec<OutputExtras>> {
        sqlx::query_as::<_, OutputExtras>(
            r#"SELECT id, has_runes, has_inscriptions
            FROM outputs_extras
            WHERE id = ANY($1)"#,
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_outputs_extras_by_rune_ids(
        &self,
        ids: &[i64],
    ) -> Result<Vec<OutputExtras>> {
        sqlx::query_as::<_, OutputExtras>(
            r#"SELECT * FROM outputs_extras
               WHERE id IN (
                  SELECT o.id FROM runes_outputs ro
                  JOIN outputs o
                  ON o.tx_hash = ro.tx_hash AND o.vout = ro.vout
                  WHERE ro.id = ANY($1)
            )"#,
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_runes_outputs(&self, tx_hash: &Hash) -> Result<Vec<RuneOutput>> {
        sqlx::query_as::<_, RuneOutput>(
            r#"SELECT
                o.id,
                o.block,
                o.tx_id,
                o.tx_hash,
                o.vout,
                o.rune,
                o.rune_id,
                a.address,
                a.pk_script,
                o.btc_amount,
                o.amount
               FROM runes_outputs o
               INNER JOIN addresses a
                  ON o.address = a.address
               WHERE o.tx_hash = $1"#,
        )
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_runes_outputs_sum(
        &self,
        address: &str,
        rune: &str,
    ) -> Result<Vec<RuneOutputsSum>> {
        sqlx::query_as::<_, RuneOutputsSum>(
            r#"SELECT
                o.block,
                o.tx_id,
                o.address,
                sum(o.btc_amount)::BIGINT as btc_amount,
                sum(o.amount) as amount,
                count(*)
               FROM runes_outputs o
               WHERE o.address = $1 AND o.rune = $2
               GROUP BY o.block, o.tx_id, o.address
               ORDER BY o.block, o.tx_id"#,
        )
        .bind(address)
        .bind(rune)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_runes_inputs_ext(&self, tx_hash: &Hash) -> Result<Vec<RuneInputFull>> {
        sqlx::query_as::<_, RuneInputFull>(
            r#" SELECT
                i.id,
                i.block,
                i.tx_id,
                i.tx_hash,
                i.vin,
                i.parent_tx,
                i.parent_vout,
                o.block as parent_block,
                o.tx_id as parent_tx_id,
                o.rune,
                o.rune_id,
                a.address,
                a.pk_script,
                o.btc_amount,
                o.amount
               FROM inputs i
               INNER JOIN runes_outputs o
                  ON i.parent_tx = o.tx_hash AND i.parent_vout = o.vout
               INNER JOIN addresses a
                  ON o.address = a.address
               WHERE i.tx_hash = $1"#,
        )
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn select_tx_runes_inputs_sum(
        &self,
        address: &str,
        rune: &str,
    ) -> Result<Vec<RuneInputsSum>> {
        sqlx::query_as::<_, RuneInputsSum>(
            r#" SELECT
                i.block,
                i.tx_id,
                o.address,
                sum(o.btc_amount)::BIGINT as btc_amount,
                sum(o.amount) as amount,
                count(*)
               FROM inputs i
               INNER JOIN runes_outputs o
                  ON i.parent_tx = o.tx_hash AND i.parent_vout = o.vout
               WHERE o.address = $1 and o.rune = $2
               GROUP BY i.block, i.tx_id, o.address ORDER BY i.block, i.tx_id"#,
        )
        .bind(address)
        .bind(rune)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn insert_api_key(&self, row: ApiKey) -> Result<()> {
        let _ = sqlx::query(
            "INSERT INTO api_keys (name, key, blocked, can_lock_utxo)
             VALUES($1, $2, $3, $4)",
        )
        .bind(row.name)
        .bind(row.key)
        .bind(row.blocked)
        .bind(row.can_lock_utxo)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn block_api_key(&self, name: &str) -> Result<()> {
        let _ = sqlx::query("UPDATE api_keys SET blocked = TRUE WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn select_api_keys(&self) -> Result<Vec<ApiKey>> {
        sqlx::query_as::<_, ApiKey>("SELECT * FROM api_keys ")
            .fetch_all(&self.pool)
            .await
    }

    pub async fn list_address_incoming_txs(
        &self,
        address: &str,
        order: OrderBy,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<AddressTx>> {
        let mut q = QueryBuilder::new(
            r#"SELECT address, block, tx_hash FROM outputs
                WHERE address = "#,
        );
        q.push_bind(address);

        q.push(" GROUP BY address, block, tx_hash ");
        q.push(format!(" ORDER BY block {order} "));
        q.push(" LIMIT ");
        q.push_bind(limit as i32);
        q.push(" OFFSET ");
        q.push_bind(offset as i32);

        let result = q
            .build_query_as::<AddressTx>()
            .fetch_all(&self.pool)
            .await?;

        Ok(result)
    }

    pub async fn list_address_outgoing_txs(
        &self,
        address: &str,
        order: OrderBy,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<AddressTx>> {
        let mut q = QueryBuilder::new(
            r#"SELECT o.address, i.block, i.tx_hash
               FROM outputs o JOIN inputs i ON i.parent_tx = o.tx_hash AND i.parent_vout = o.vout
               WHERE o.address = "#,
        );
        q.push_bind(address);

        q.push(" GROUP by o.address, i.block, i.tx_hash ");
        q.push(format!(" ORDER BY i.block {order} "));
        q.push(" LIMIT ");
        q.push_bind(limit as i32);
        q.push(" OFFSET ");
        q.push_bind(offset as i32);

        let result = q
            .build_query_as::<AddressTx>()
            .fetch_all(&self.pool)
            .await?;

        Ok(result)
    }
}
