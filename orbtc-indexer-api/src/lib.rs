use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlx")]
use sqlx::prelude::FromRow;

pub mod btc;
pub mod runes;
pub mod types;

pub use api_core::pages::OrderBy;
pub use btc::*;
pub use runes::*;
pub use types::{Amount, Hash};

#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub enum UtxoSortMode {
    #[serde(rename = "age", alias = "AGE")]
    Age,

    #[default]
    #[serde(rename = "amount", alias = "AMOUNT")]
    Amount,
}

impl std::str::FromStr for UtxoSortMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "amount" => Ok(Self::Amount),
            "age" => Ok(Self::Age),
            _ => Err(anyhow::anyhow!(
                "invalid utxo_sort_mode: possible values are `age` or `amount`"
            )),
        }
    }
}

impl std::fmt::Display for UtxoSortMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Age => write!(f, "age"),
            Self::Amount => write!(f, "amount"),
        }
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct LastIndexedBlock {
    pub indexer: String,
    pub height: i64,
}

#[derive(Default, Clone, Copy, Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub healthy: bool,
    pub db: bool,
    pub btc_node: bool,
    pub btc_height: u64,
    pub btc_indexer: bool,
    pub btc_indexer_height: u64,
    pub runes_indexer: bool,
    pub runes_indexer_height: u64,
}

#[derive(Debug, Clone)]
pub enum Utxo {
    Btc(BtcUtxo),
    Rune(RuneUtxo),
}

impl Utxo {
    pub fn out_point(&self) -> bitcoin::OutPoint {
        match self {
            Utxo::Btc(u) => u.out_point(),
            Utxo::Rune(u) => u.out_point(),
        }
    }

    pub fn btc_amount(&self) -> i64 {
        match self {
            Utxo::Btc(u) => u.amount,
            Utxo::Rune(u) => u.btc_amount,
        }
    }

    pub fn btc_utxo(&self) -> BtcUtxo {
        match self {
            Utxo::Btc(u) => u.clone(),
            Utxo::Rune(u) => BtcUtxo {
                id: u.id,
                block: u.block,
                tx_id: u.tx_id,
                tx_hash: u.tx_hash.clone(),
                vout: u.vout,
                address: u.address.clone(),
                pk_script: u.pk_script.clone(),
                amount: u.btc_amount,
                spend: false,
            },
        }
    }
    pub fn amount(&self) -> u128 {
        use bigdecimal::ToPrimitive;
        match self {
            Utxo::Btc(u) => u.amount as u128,
            Utxo::Rune(u) => u.amount.to_u128().expect("rune amount in out of range"),
        }
    }
}
