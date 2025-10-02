use bitcoin::OutPoint;
use diesel;
use diesel::prelude::*;
use orbtc_indexer_api::types::{Amount, Hash};
use ordinals::{RuneId, Runestone, Terms};

#[derive(Default, Clone, Debug, Queryable, Selectable, Insertable)]
#[diesel(table_name = tables::last_indexed_block, primary_key(indexer))]
pub struct LastIndexedBlock {
    pub indexer: String,
    pub height: i64,
}

#[derive(Default, Clone, Debug, Queryable, Selectable, Insertable)]
#[diesel(table_name = tables::blocks, primary_key(height))]
pub struct Block {
    pub height: i64,
    pub hash: Hash,
    pub blocktime: i64,
    pub indexer: String,
}

#[derive(Default, Clone, Debug, Queryable, Selectable, Insertable)]
#[diesel(table_name = tables::addresses)]
pub struct Address {
    #[diesel(deserialize_as = i64)]
    pub id: Option<i64>,
    pub address: String,
    pub address_type: String,
    pub pk_script: Vec<u8>,
}

#[derive(
    Default, Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Queryable, Selectable, Insertable,
)]
#[diesel(table_name = tables::outputs)]
pub struct Output {
    #[diesel(deserialize_as = i64)]
    pub id: Option<i64>,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vout: i32,
    pub address: String,
    pub amount: i64,
    pub coinbase: bool,
}

impl Output {
    pub fn out_point(&self) -> OutPoint {
        OutPoint {
            txid: (&self.tx_hash).into(),
            vout: self.vout as u32,
        }
    }
}

#[derive(
    Default, Clone, Debug, PartialEq, PartialOrd, Ord, Eq, Queryable, Selectable, Insertable,
)]
#[diesel(table_name = tables::inputs)]
pub struct Input {
    #[diesel(deserialize_as = i64)]
    pub id: Option<i64>,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vin: i32,
    pub parent_tx: Hash,
    pub parent_vout: i32,
}

#[derive(Default, Clone, Debug, Queryable, Selectable, Insertable)]
#[diesel(table_name = tables::runes, primary_key(block, tx_id))]
pub struct Rune {
    pub block: i64,
    pub tx_id: i32,
    pub rune_id: String,
    pub name: String,
    pub display_name: String,
    pub symbol: String,
    pub mints: i32,
    pub max_supply: Amount,
    pub premine: Amount,
    pub burned: Amount,
    pub minted: Amount,
    pub in_circulation: Amount,
    pub divisibility: i32,
    pub turbo: bool,
    pub cenotaph: bool,
    pub block_time: i64,
    pub etching_tx: Hash,
    pub commitment_tx: Hash,
    pub raw_data: Vec<u8>,
    pub is_featured: bool,
}

impl Rune {
    pub fn add_mint(&mut self, amount: u128) -> bool {
        self.mints += 1;
        self.in_circulation.0 += amount;
        self.minted.0 += amount;
        true
    }

    pub fn burn(&mut self, amount: u128) -> bool {
        if amount > self.in_circulation.0 {
            return false;
        }

        self.burned.0 += amount;
        self.in_circulation.0 -= amount;
        true
    }

    pub fn rune_id(&self) -> RuneId {
        RuneId {
            block: self.block as u64,
            tx: self.tx_id as u32,
        }
    }

    pub fn terms(&self) -> Option<Terms> {
        if self.raw_data.is_empty() {
            return None;
        }
        let runestone: Runestone = serde_json::from_slice(&self.raw_data).ok()?;
        runestone.etching.and_then(|e| e.terms)
    }
}

#[derive(Default, Clone, Debug)]
pub struct RuneShortRow {
    pub rune: String,
    pub block: i64,
    pub tx_id: i32,
    pub mints: i32,
    pub minted: Amount,
    pub in_circulation: Amount,
    pub raw_data: Vec<u8>,
}

#[derive(
    Default, Clone, Debug, Queryable, Selectable, Insertable, PartialEq, PartialOrd, Ord, Eq,
)]
#[diesel(table_name = tables::runes_outputs)]
pub struct RuneUtxo {
    #[diesel(deserialize_as = i64)]
    pub id: Option<i64>,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vout: i32,
    pub rune: String,
    pub rune_id: String,
    pub address: String,
    pub amount: Amount,
    pub btc_amount: i64,
}

impl RuneUtxo {
    pub fn out_point(&self) -> OutPoint {
        OutPoint {
            txid: (&self.tx_hash).into(),
            vout: self.vout as u32,
        }
    }
}

#[derive(
    Default, Clone, Debug, Queryable, Selectable, Insertable, PartialEq, PartialOrd, Ord, Eq,
)]
#[diesel(table_name = tables::outputs_runes_ext)]
pub struct OutputRuneExt {
    pub id: i64,
    pub rune: String,
    pub rune_id: String,
    pub rune_amount: Amount,
}

#[derive(
    Default, Clone, Debug, Queryable, Selectable, Insertable, PartialEq, PartialOrd, Ord, Eq,
)]
#[diesel(table_name = tables::outputs_extras)]
pub struct OutputExtras {
    pub id: i64,
    pub has_runes: bool,
    pub has_inscriptions: bool,
}

pub mod tables {
    use diesel::prelude::*;

    table! {
       last_indexed_block(indexer) {
           indexer -> VarChar,
           height -> BigInt,
       }
    }

    table! {
       blocks(height) {
           height -> BigInt,
           hash -> Bytea,
           blocktime -> BigInt,
           indexer -> VarChar,
       }
    }

    table! {
        addresses {
            id -> BigSerial,
            address -> Varchar,
            address_type -> Varchar,
            pk_script -> Bytea,
        }
    }

    table! {
        outputs {
            id -> BigSerial,
            block -> BigInt,
            tx_id -> Integer,
            tx_hash -> Bytea,
            vout -> Integer,
            address -> VarChar,
            amount -> BigInt,
            coinbase -> Bool,
        }
    }

    table! {
        inputs {
            id -> BigSerial,
            block -> BigInt,
            tx_id -> Integer,
            tx_hash -> Bytea,
            vin -> Integer,
            parent_tx -> Bytea,
            parent_vout -> Integer,
        }
    }

    table! {
        runes (block, tx_id) {
            block -> BigInt,
            tx_id -> Integer,
            rune_id -> VarChar,
            name -> VarChar,
            display_name -> VarChar,
            symbol -> VarChar,
            divisibility -> Integer,
            mints -> Integer,
            max_supply -> Numeric,
            premine -> Numeric,
            burned -> Numeric,
            minted -> Numeric,
            in_circulation -> Numeric,
            turbo -> Bool,
            cenotaph -> Bool,
            block_time -> BigInt,
            etching_tx -> Bytea,
            commitment_tx -> Bytea,
            raw_data -> Bytea,
            is_featured -> Bool,
        }
    }

    table! {
        runes_outputs {
            id -> BigSerial,
            block -> BigInt,
            tx_id -> Integer,
            tx_hash -> Bytea,
            vout -> Integer,
            rune -> VarChar,
            rune_id -> VarChar,
            address -> VarChar,
            amount -> Numeric,
            btc_amount -> BigInt,
        }
    }

    table! {
        outputs_runes_ext {
            id -> BigSerial,
            rune -> VarChar,
            rune_id -> VarChar,
            rune_amount -> Numeric,
        }
    }

    table! {
        outputs_extras {
            id -> BigSerial,
            has_runes -> Bool,
            has_inscriptions -> Bool
        }
    }
}
