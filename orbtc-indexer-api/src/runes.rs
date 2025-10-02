use api_core::pages::PageParams;
use api_core::serde_utils::{bigdecimal_plain_str, bytevec_as_hex};
use bigdecimal::BigDecimal;
use bitcoin::script::Builder;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness};
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlx")]
use sqlx::prelude::FromRow;

use super::types::Hash;
use super::UtxoSortMode;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Rune {
    pub block: i64,
    pub tx_id: i32,
    pub rune_id: String,
    pub name: String,
    pub display_name: String,
    pub symbol: String,
    pub mints: i32,
    #[serde(with = "bigdecimal_plain_str")]
    pub max_supply: BigDecimal,
    #[serde(with = "bigdecimal_plain_str")]
    pub premine: BigDecimal,
    #[serde(with = "bigdecimal_plain_str")]
    pub burned: BigDecimal,
    #[serde(with = "bigdecimal_plain_str")]
    pub minted: BigDecimal,
    #[serde(with = "bigdecimal_plain_str")]
    pub in_circulation: BigDecimal,
    pub divisibility: i32,
    pub turbo: bool,
    pub block_time: i64,
    pub etching_tx: Hash,
    pub commitment_tx: Hash,
    #[serde(with = "bytevec_as_hex")]
    pub raw_data: Vec<u8>,
    pub is_featured: bool,
}

impl Rune {
    pub fn to_rune_id(&self) -> ordinals::RuneId {
        ordinals::RuneId {
            block: self.block as u64,
            tx: self.tx_id as u32,
        }
    }
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneUtxo {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vout: i32,
    pub rune: String,
    pub rune_id: String,
    pub address: String,
    #[serde(with = "bytevec_as_hex")]
    pub pk_script: Vec<u8>,
    #[serde(with = "bigdecimal_plain_str")]
    pub amount: BigDecimal,
    pub btc_amount: i64,
}

impl RuneUtxo {
    pub fn out_point(&self) -> OutPoint {
        OutPoint {
            txid: (&self.tx_hash).into(),
            vout: self.vout as u32,
        }
    }
    pub fn into_tx_parent(&self) -> (TxIn, TxOut) {
        let parent_in = TxIn {
            previous_output: OutPoint {
                txid: (&self.tx_hash).into(),
                vout: self.vout as u32,
            },
            script_sig: Builder::new().into_script(),
            witness: Witness::new(),
            sequence: Sequence::MAX,
        };

        let parent_out = TxOut {
            script_pubkey: ScriptBuf::from_bytes(self.pk_script.clone()),
            value: Amount::from_sat(self.btc_amount as u64),
        };

        (parent_in, parent_out)
    }
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneBalance {
    pub address: String,
    pub rune: String,
    pub rune_id: String,
    pub divisibility: i32,
    pub symbol: String,
    #[serde(with = "bigdecimal_plain_str")]
    pub balance: BigDecimal,
    pub btc_balance: i64,
    pub utxo_count: i64,
}

impl RuneBalance {
    pub fn get_rune_id(&self) -> ordinals::RuneId {
        let Some((block, tx)) = self.rune_id.split_once(":") else {
            return ordinals::RuneId::default();
        };

        let block = block.parse().unwrap_or_default();
        let tx = tx.parse().unwrap_or_default();
        ordinals::RuneId { block, tx }
    }
}
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneBalanceHistory {
    pub block: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub rune_balance: BigDecimal,
    pub btc_balance: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub rune_income: BigDecimal,
    pub btc_income: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub rune_spent: BigDecimal,
    pub btc_spent: i64,
    pub in_count: i64,
    pub out_count: i64,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct ListRunesQuery {
    #[serde(flatten)]
    pub page: PageParams,
    pub name: Option<String>,
    pub featured: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchQuery {
    pub s: String,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct RunesFilter {
    pub runes: Vec<String>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct RunesUtxoQuery {
    #[serde(flatten)]
    pub page: PageParams,
    #[serde(default)]
    pub sorting: UtxoSortMode,
    pub amount_threshold: Option<u64>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct RunesHoldersQuery {
    #[serde(flatten)]
    pub page: PageParams,
    pub amount_threshold: Option<u64>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct CollectRunesUtxo {
    #[serde(with = "bigdecimal_plain_str")]
    pub amount: BigDecimal,
    pub request_id: String,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneInputFull {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vin: i32,
    pub parent_tx: Hash,
    pub parent_vout: i32,
    pub parent_block: i64,
    pub parent_tx_id: i32,
    pub rune: String,
    pub rune_id: String,
    pub address: String,
    #[serde(with = "bytevec_as_hex")]
    pub pk_script: Vec<u8>,
    pub btc_amount: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub amount: BigDecimal,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneInputsSum {
    pub block: i64,
    pub tx_id: i32,
    pub address: String,
    pub btc_amount: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub amount: BigDecimal,
    pub count: i64,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneOutput {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vout: i32,
    pub rune: String,
    pub rune_id: String,
    pub address: String,
    #[serde(with = "bytevec_as_hex")]
    pub pk_script: Vec<u8>,
    pub btc_amount: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub amount: BigDecimal,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct RuneOutputsSum {
    pub block: i64,
    pub tx_id: i32,
    pub address: String,
    pub btc_amount: i64,
    #[serde(with = "bigdecimal_plain_str")]
    pub amount: BigDecimal,
    pub count: i64,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct RuneTxInOuts {
    pub inputs: Vec<RuneInputFull>,
    pub outputs: Vec<RuneOutput>,
}
