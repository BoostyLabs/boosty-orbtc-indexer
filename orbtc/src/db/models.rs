use api_core::serde_utils::bytevec_as_hex;
use bigdecimal::BigDecimal;
use orbtc_indexer_api::types::Hash;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;

#[derive(Default, Clone, Debug, FromRow, Serialize, Deserialize)]
pub struct RuneShortRow {
    pub rune: String,
    pub block: i64,
    pub tx_id: i32,
    pub mints: i32,
    pub minted: BigDecimal,
    pub in_circulation: BigDecimal,

    #[serde(with = "bytevec_as_hex")]
    pub raw_data: Vec<u8>,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct ShortTxOut {
    pub tx_hash: Hash,
    pub vout: i32,
}

#[derive(Default, Clone, Debug, FromRow, Serialize)]
pub struct OutputExtras {
    pub id: i64,
    pub has_runes: bool,
    pub has_inscriptions: bool,
}

#[derive(Default, Clone, Debug, FromRow, Serialize, Deserialize)]
pub struct AddressTx {
    pub address: String,
    pub tx_hash: Hash,
    pub block: i64,
}

#[derive(Default, Clone, Debug, FromRow, Serialize, Deserialize)]
pub struct ApiKey {
    pub name: String,
    pub key: String,
    pub blocked: bool,
    pub can_lock_utxo: bool,
}

impl ApiKey {
    pub fn new(name: &str) -> Self {
        use base64::Engine;
        use rand::Rng;

        let mut rng = rand::thread_rng();
        let key: Vec<u8> = (0..24).map(|_| rng.gen::<u8>()).collect();
        let key = base64::prelude::BASE64_URL_SAFE.encode(key);
        Self {
            name: name.into(),
            key,
            blocked: false,
            can_lock_utxo: false,
        }
    }
}
