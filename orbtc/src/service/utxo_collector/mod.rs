use std::sync::Arc;

pub use algo::{min_utxos_to_reach_target, KnapsackError};
use async_trait::async_trait;
use bigdecimal::ToPrimitive;
use orbtc_indexer_api::{BtcUtxo, OrderBy, RuneUtxo, UtxoSortMode};

use crate::db::Repo;

mod algo;

#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    #[error("Not enough balance. Available: {available}, Required: {target}")]
    NotEnoughBalance { available: u128, target: u128 },

    #[error("Top {max} biggest UTXOs are not enough to collect {target} amount (collected={collected}). Total UTXOs={total_utxo}")]
    NeedMoreUtxos {
        total_utxo: u32,
        max: u32,
        collected: u128,
        target: u128,
    },

    #[error("Bad input: {0}")]
    BadInput(String),

    #[error("DB error: {0}")]
    DbError(#[from] sqlx::Error),
}

impl algo::Utxo for BtcUtxo {
    fn get_amount(&self) -> u128 {
        self.amount as u128
    }
}

impl algo::Utxo for RuneUtxo {
    fn get_amount(&self) -> u128 {
        self.amount.to_u128().expect("amount must fit u128")
    }
}

#[async_trait]
pub trait UtxoCollector: Send + Sync {
    /// collect RUNE UTXOs for a given address and rune.
    /// If Ok is returned, it is guaranteed that the sum of the UTXOs is >= target
    /// and len(utxos) <= max_utxos.
    async fn collect_btc_utxo(
        &self,
        address: &str,
        target: u64,
        max_utxos: u32,
    ) -> Result<Vec<BtcUtxo>, CollectorError>;

    /// collect RUNE UTXOs for a given address and rune.
    /// If Ok is returned, it is guaranteed that the sum of the UTXOs is >= target
    /// and len(utxos) <= max_utxos.
    async fn collect_rune_utxo(
        &self,
        address: &str,
        rune: &str,
        target: u128,
        max_utxos: u32,
    ) -> Result<Vec<RuneUtxo>, CollectorError>;
}

/// This service is responsible for collecting UTXOs for a given address/rune.
///
#[derive(Debug, Clone)]
pub struct UtxoCollectorService {
    db: Arc<Repo>,
}

impl UtxoCollectorService {
    pub fn new(db: Arc<Repo>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl UtxoCollector for UtxoCollectorService {
    async fn collect_btc_utxo(
        &self,
        address: &str,
        target: u64,
        mut max_utxos: u32,
    ) -> Result<Vec<BtcUtxo>, CollectorError> {
        if target == 0 {
            return Err(CollectorError::BadInput(
                "Target amount is zero".to_string(),
            ));
        }

        // 1k is a reasonable upper limit for the number of UTXOs to consider
        max_utxos = max_utxos.clamp(1, 1000);

        // shortcut: if user's balance is already "not enough", return early
        let balance = self
            .db
            .get_balance(address)
            .await
            .map_err(CollectorError::DbError)?;
        if (balance.balance as u64) < target {
            return Err(CollectorError::NotEnoughBalance {
                available: balance.balance as u128,
                target: target.into(),
            });
        }

        // shortcut: is there 1 UTXO that is >= than target? If yes, pick it and return early.
        if let Some(utxo) = self
            .db
            .get_address_btc_utxo_ge_amount(address, target)
            .await
            .map_err(CollectorError::DbError)?
        {
            return Ok(vec![utxo]);
        }

        // at this point we know that the user has enough balance,
        let candidates = self
            .db
            .select_utxo_with_pagination(
                address,
                OrderBy::Desc,
                Some(800),
                None, // TODO(Bohdan): must skip immature UTXOs as they are not spendable
                UtxoSortMode::Amount,
                max_utxos,
                0,
            )
            .await
            .map_err(CollectorError::DbError)?;

        match min_utxos_to_reach_target(&candidates, target.into()) {
            Ok(utxos) => Ok(utxos),
            Err(KnapsackError::NotEnoughBalance { available, target }) => {
                Err(CollectorError::NotEnoughBalance { available, target })
            }
        }
    }

    async fn collect_rune_utxo(
        &self,
        address: &str,
        rune: &str,
        target: u128,
        mut max_utxos: u32,
    ) -> Result<Vec<RuneUtxo>, CollectorError> {
        if target == 0 {
            return Err(CollectorError::BadInput(
                "Target amount is zero".to_string(),
            ));
        }

        // 1k is a reasonable upper limit for the number of UTXOs to consider
        max_utxos = max_utxos.clamp(1, 1000);

        // shortcut: if user's balance is already "not enough", return early
        let balance = self
            .db
            .get_rune_balance(address, rune)
            .await
            .map_err(CollectorError::DbError)?
            .balance
            .to_u128()
            .expect("balance must fit u128");

        if balance < target {
            return Err(CollectorError::NotEnoughBalance {
                available: balance,
                target,
            });
        }

        // shortcut: is there 1 UTXO that is >= than target? If yes, pick it and return early.
        if let Some(utxo) = self
            .db
            .get_address_rune_utxo_ge_amount(address, rune, target)
            .await
            .map_err(CollectorError::DbError)?
        {
            return Ok(vec![utxo]);
        }

        // at this point we know that the user has enough balance,
        let candidates = self
            .db
            .select_rune_utxo_with_pagination(
                rune,
                address,
                OrderBy::Desc,
                Some(800),
                orbtc_indexer_api::UtxoSortMode::Amount,
                max_utxos,
                0,
            )
            .await
            .map_err(CollectorError::DbError)?;

        match min_utxos_to_reach_target(&candidates, target) {
            Ok(utxos) => Ok(utxos),
            Err(KnapsackError::NotEnoughBalance { available, target }) => {
                Err(CollectorError::NotEnoughBalance { available, target })
            }
        }
    }
}
