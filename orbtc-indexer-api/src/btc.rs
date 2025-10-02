use std::collections::HashMap;
use std::str::FromStr;

use actix_web::http::StatusCode;
use actix_web::{HttpResponse, ResponseError};
use api_core::api_errors::*;
use api_core::pages::PageParams;
use api_core::serde_utils::bytevec_as_hex;
use bitcoin::script::Builder;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Txid, Witness};
use serde::{Deserialize, Serialize};
#[cfg(feature = "sqlx")]
use sqlx::prelude::FromRow;

use super::types::Hash;
use super::UtxoSortMode;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Balance {
    pub address: String,
    pub balance: i64,
    pub utxo_count: i64,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct BtcBalanceHistoryPoint {
    pub block: i64,
    pub balance: i64,
    pub income: i64,
    pub spent: i64,
    pub in_count: i64,
    pub out_count: i64,
}

#[derive(Default, Clone, Debug, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct Input {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: String,
    pub vin: i32,
    pub parent_tx: String,
    pub parent_vout: i32,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct InputFull {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vin: i32,
    pub parent_tx: Hash,
    pub parent_vout: i32,
    pub parent_block: i64,
    pub parent_tx_id: i32,
    pub address: String,
    #[serde(with = "bytevec_as_hex")]
    pub pk_script: Vec<u8>,
    pub amount: i64,
    pub coinbase: bool,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct InputsSum {
    pub block: i64,
    pub tx_id: i32,
    pub address: String,
    pub amount: i64,
    pub count: i64,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct BtcOutput {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vout: i32,
    pub address: String,
    #[serde(with = "bytevec_as_hex")]
    pub pk_script: Vec<u8>,
    pub amount: i64,
    pub coinbase: bool,
    pub spend: bool,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct BtcOutputsSum {
    pub block: i64,
    pub tx_id: i32,
    pub address: String,
    pub amount: i64,
    pub count: i64,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct BtcUtxo {
    pub id: i64,
    pub block: i64,
    pub tx_id: i32,
    pub tx_hash: Hash,
    pub vout: i32,
    pub address: String,
    #[serde(with = "bytevec_as_hex")]
    pub pk_script: Vec<u8>,
    pub amount: i64,
    /// DEPRECATED
    pub spend: bool,
}

impl BtcUtxo {
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
            value: Amount::from_sat(self.amount as u64),
        };

        (parent_in, parent_out)
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct UtxoQuery {
    #[serde(flatten)]
    pub page: PageParams,
    #[serde(default)]
    pub sorting: UtxoSortMode,
    pub amount_threshold: Option<u64>,
    #[serde(default)]
    pub skip_premature: bool,
    #[serde(default)]
    pub no_runes: bool,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct CollectUtxo {
    pub amount: u64,
    pub request_id: String,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct TxInOuts {
    // TODO: add this fields
    // pub heigh: u64,
    // pub block_hash: String,
    // pub blocktime: u64,
    // pub raw_tx: Option<String>,
    // pub network_fee: u64,
    pub inputs: Vec<InputFull>,
    pub outputs: Vec<BtcOutput>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct ListTxQuery {
    #[serde(flatten)]
    pub page: PageParams,
    #[serde(default)]
    pub min_height: Option<u64>,
    #[serde(default)]
    pub mempool: bool,
    #[serde(default)]
    pub incoming_only: bool,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "sqlx", derive(FromRow))]
pub struct TxInfo {
    pub tx_hash: Hash,
    pub block: i64,
    pub income: bool,
    pub spend: bool,
}

#[derive(Clone, PartialEq, Eq, Debug, Deserialize, Serialize)]
pub struct RawTxInfo {
    pub in_active_chain: Option<bool>,
    pub confirmations: Option<u32>,
    pub time: Option<usize>,
    pub blocktime: Option<usize>,
    /// a transaction block hash
    pub blockhash: Option<String>,
    /// a height of a block
    pub blockheight: Option<usize>,
    /// a position of a transaction in a block
    pub txnumber: Option<usize>,
    pub raw_tx: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Box<serde_json::value::RawValue>>,
}

impl PartialEq for RpcError {
    fn eq(&self, other: &Self) -> bool {
        if !(self.code == other.code && self.message == other.message) {
            return false;
        }

        // RawData is not == comparable :(
        match (&self.data, &other.data) {
            (Some(self_data), Some(other_data)) => self_data.get().eq(other_data.get()),
            (None, None) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTxRequest {
    pub tx: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxHash {
    pub tx_hash: String,
}

pub type SendTxResponse = Response<TxHash, RpcError>;

/// This allows us to return error as a value, not as "api error".
#[derive(Debug, Serialize, Deserialize)]
pub struct Response<T, E> {
    pub result: Option<T>,
    pub error: Option<E>,
}

pub type GetMempoolTxsResponse = Response<Vec<Txid>, RpcError>;

pub type GetTxResponse = Response<RawTxInfo, RpcError>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum FBtcApiError {
    #[error("something went wrong")]
    InternalError,

    #[error("address is invalid: {0}")]
    InvalidAddress(String),

    #[error("bad input: {0}")]
    BadInput(String),

    #[error("not enough balance: required={required}, available={available}")]
    NotEnoughBalance { required: u128, available: u128 },

    // TODO(Bohdan): we probably want to pass these args to caller so that this can be shown to end user.
    #[error("Top {max} biggest UTXOs are not enough to collect {target} amount (collected={collected}). Total UTXOs={total_utxos}")]
    NeedMoreUtxos {
        max: u32,
        total_utxos: u32,
        target: u128,
        collected: u128,
    },

    #[error("service is temporaly unavailable; check the /status response")]
    ServiceUnavailable,

    #[error("not found")]
    NotFound,

    #[error("unauthorized")]
    Unauthorized,

    #[error("forbidden; api-key is blocked")]
    Forbidden,
}

impl TryFrom<&ApiError> for FBtcApiError {
    type Error = anyhow::Error;
    fn try_from(error: &ApiError) -> Result<Self, Self::Error> {
        use FBtcApiError::*;
        let code = ApiErrorCode::try_from(error.code)?;
        Ok(match code {
            ApiErrorCode::InternalError => InternalError,
            ApiErrorCode::AccessDenied => Unauthorized,
            ApiErrorCode::Forbidden => Forbidden,
            ApiErrorCode::ServiceUnavailable => ServiceUnavailable,
            ApiErrorCode::InvalidAddress => InvalidAddress(error.message.clone()),
            ApiErrorCode::BadInput => BadInput(error.message.clone()),
            ApiErrorCode::NotFound => NotFound,
            ApiErrorCode::NeedMoreUtxos => NeedMoreUtxos {
                max: 0,
                total_utxos: 0,
                target: 0,
                collected: 0,
            },
            ApiErrorCode::NotEnoughBalance => {
                let required = error
                    .details
                    .get("required")
                    .and_then(|v| u128::from_str(v).ok())
                    .unwrap_or_default();
                let available = error
                    .details
                    .get("available")
                    .and_then(|v| u128::from_str(v).ok())
                    .unwrap_or_default();

                NotEnoughBalance {
                    required,
                    available,
                }
            }
        })
    }
}

impl From<&FBtcApiError> for ApiError {
    fn from(error: &FBtcApiError) -> ApiError {
        use FBtcApiError::*;
        let mut details = HashMap::new();
        let code = match error {
            Unauthorized => ApiErrorCode::AccessDenied,
            Forbidden => ApiErrorCode::Forbidden,
            InternalError => ApiErrorCode::InternalError,
            ServiceUnavailable => ApiErrorCode::ServiceUnavailable,
            InvalidAddress(_) => ApiErrorCode::InvalidAddress,
            BadInput(_) => ApiErrorCode::BadInput,
            NotFound => ApiErrorCode::NotFound,
            NotEnoughBalance {
                required,
                available,
            } => {
                details.insert("required".into(), required.to_string());
                details.insert("available".into(), available.to_string());
                ApiErrorCode::NotEnoughBalance
            }
            NeedMoreUtxos {
                max,
                total_utxos,
                target,
                collected,
            } => {
                details.insert("max".into(), max.to_string());
                details.insert("total_utxos".into(), total_utxos.to_string());
                details.insert("target".into(), target.to_string());
                details.insert("collected".into(), collected.to_string());
                ApiErrorCode::NeedMoreUtxos
            }
        };
        ApiError {
            code: code as u16,
            status: code.to_string(),
            http_code: error.status_code(),
            message: error.to_string(),
            details,
        }
    }
}

impl ResponseError for FBtcApiError {
    fn status_code(&self) -> StatusCode {
        use FBtcApiError::*;
        match self {
            Unauthorized => StatusCode::UNAUTHORIZED,
            Forbidden => StatusCode::FORBIDDEN,
            InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            InvalidAddress(_) => StatusCode::BAD_REQUEST,
            BadInput(_) => StatusCode::BAD_REQUEST,
            NotFound => StatusCode::NOT_FOUND,
            NeedMoreUtxos { .. } | NotEnoughBalance { .. } => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        ApiError::from(self).into()
    }
}
