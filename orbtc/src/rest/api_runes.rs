use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;

use actix_web::http::StatusCode;
use actix_web::web::{Data, Json, Path, Query};
use actix_web::{HttpResponse, ResponseError};
use api_core::api_errors::*;
use api_core::pages::{ListResponseMeta, ListResult};
use bigdecimal::{BigDecimal, ToPrimitive};
use orbtc_indexer_api::{types, *};
use serde::{Deserialize, Serialize};

use super::context::Context;
use super::requests::decode_address;
use crate::service::utxo_collector::{min_utxos_to_reach_target, KnapsackError};

#[derive(Debug, thiserror::Error)]
pub enum RuneApiError {
    #[error("something went wrong")]
    InternalError,
    #[error("service is temporaly unavailable; check the /status response")]
    ServiceUnavailable,
    #[error("rune with this name not exist: {0}")]
    NotFound(String),
    #[error("rune name is invalid: {0}")]
    InvalidRuneName(String),
    #[error("address is invalid: {0}")]
    InvalidAddress(String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("not enough balance: required={required}, available={available}")]
    NotEnoughBalance { required: u128, available: u128 },
}

impl From<&RuneApiError> for ApiError {
    fn from(error: &RuneApiError) -> ApiError {
        use RuneApiError::*;
        let mut details = HashMap::new();
        let code = match error {
            Unauthorized => ApiErrorCode::AccessDenied,
            InternalError => ApiErrorCode::InternalError,
            ServiceUnavailable => ApiErrorCode::ServiceUnavailable,
            NotFound(_) => ApiErrorCode::NotFound,
            InvalidRuneName(_) => ApiErrorCode::BadInput,
            InvalidAddress(_) => ApiErrorCode::BadInput,
            BadInput(_) => ApiErrorCode::BadInput,
            NotEnoughBalance {
                required,
                available,
            } => {
                details.insert("required".into(), required.to_string());
                details.insert("available".into(), available.to_string());
                ApiErrorCode::NotEnoughBalance
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

impl ResponseError for RuneApiError {
    fn status_code(&self) -> actix_web::http::StatusCode {
        use RuneApiError::*;
        match self {
            Unauthorized => StatusCode::UNAUTHORIZED,
            InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            NotFound(_) => StatusCode::NOT_FOUND,
            InvalidRuneName(_) => StatusCode::BAD_REQUEST,
            InvalidAddress(_) => StatusCode::BAD_REQUEST,
            BadInput(_) => StatusCode::BAD_REQUEST,
            NotEnoughBalance { .. } => StatusCode::BAD_REQUEST,
        }
    }

    fn error_response(&self) -> HttpResponse {
        ApiError::from(self).into()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RuneAddressPath {
    pub address: String,
    pub rune: String,
}

pub async fn search_runes(
    state: Data<Context>,
    params: Query<SearchQuery>,
) -> Result<Json<ListResult<Rune>>, RuneApiError> {
    let res = state.db.search_runes(&params.s).await;
    match res {
        Ok(runes_rows) => {
            let resp = ListResult {
                meta: None,
                records: runes_rows,
            };

            Ok(Json(resp))
        }
        Err(err) => {
            error!("status request failed: error={:#?}", err);
            Err(RuneApiError::InternalError)
        }
    }
}

pub async fn list_runes(
    state: Data<Context>,
    params: Query<ListRunesQuery>,
) -> Result<Json<ListResult<Rune>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let (limit, offset) = match params.page.limit_offset() {
        Ok(v) => v,
        Err(err) => {
            return Err(RuneApiError::BadInput(format!("{err}")));
        }
    };

    let mut name_filter = None;
    if let Some(name) = params.name.clone() {
        let name = name.to_ascii_uppercase(); // support search by lowercase
        match ordinals::SpacedRune::from_str(&name) {
            Ok(spr) => {
                name_filter = Some(spr.rune.to_string());
            }
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        };
    }

    let count_res = state
        .db
        .count_runes(name_filter.clone(), params.featured)
        .await;

    let count = match count_res {
        Ok(count) => count,
        Err(err) => {
            error!("can't count runes: error={:#?}", err);
            0
        }
    };

    let res = state
        .db
        .list_runes(
            params.page.order,
            limit,
            offset,
            name_filter,
            params.featured,
        )
        .await;

    match res {
        Ok(runes_rows) => {
            let resp = ListResult {
                meta: Some(ListResponseMeta::new(limit, offset, count as u64)),
                records: runes_rows,
            };

            Ok(Json(resp))
        }
        Err(err) => {
            error!("status request failed: error={:#?}", err);
            Err(RuneApiError::InternalError)
        }
    }
}

pub async fn get_rune(
    state: Data<Context>,
    rune: Path<String>,
) -> Result<Json<Rune>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let name_filter = {
        match ordinals::SpacedRune::from_str(&rune) {
            Ok(spr) => spr.rune.to_string(),
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        }
    };

    let res = state.db.get_rune(&name_filter).await;
    match res {
        Ok(Some(row)) => Ok(Json(row)),
        Ok(None) => Err(RuneApiError::NotFound(rune.to_owned())),
        Err(err) => {
            error!("can't fetch rune by name: rune={rune} error={:#?}", err);
            Err(RuneApiError::InternalError)
        }
    }
}

pub async fn list_rune_holders(
    state: Data<Context>,
    rune: Path<String>,
    query: Query<RunesHoldersQuery>,
) -> Result<Json<ListResult<RuneBalance>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let (limit, offset) = match query.page.limit_offset() {
        Ok(v) => v,
        Err(err) => {
            return Err(RuneApiError::BadInput(format!("{err}")));
        }
    };

    let rune = {
        match ordinals::SpacedRune::from_str(&rune) {
            Ok(spr) => spr.rune.to_string(),
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        }
    };

    let res = state
        .db
        .get_rune_holders(
            &rune,
            query.page.order,
            query.amount_threshold,
            limit,
            offset,
        )
        .await;

    let balances = match res {
        Ok(balances) => balances,
        Err(err) => {
            error!("can't fetch rune holders: rune={rune} error={:#?}", err);
            return Err(RuneApiError::InternalError);
        }
    };

    Ok(Json(ListResult {
        records: balances,
        meta: Some(ListResponseMeta::new(limit, offset, 0)),
    }))
}

pub async fn get_rune_balance(
    state: Data<Context>,
    params: Path<RuneAddressPath>,
) -> Result<Json<RuneBalance>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let address = params.address.clone();
    let rune = {
        match ordinals::SpacedRune::from_str(&params.rune) {
            Ok(spr) => spr.rune.to_string(),
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        }
    };

    let res = state.db.get_rune_balance(&address, &rune).await;

    let balance = match res {
        Ok(balances) => balances,
        Err(err) => {
            error!(
                "can't fetch rune balances: address={address} rune={rune} error={:#?}",
                err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    Ok(Json(balance))
}

pub async fn get_rune_balance_history(
    state: Data<Context>,
    params: Path<RuneAddressPath>,
) -> Result<Json<Vec<RuneBalanceHistory>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let rune = {
        match ordinals::SpacedRune::from_str(&params.rune) {
            Ok(spr) => spr.rune.to_string(),
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        }
    };

    let outputs = match state
        .db
        .select_tx_runes_outputs_sum(&params.address, &rune)
        .await
    {
        Ok(o) => o,
        Err(err) => {
            error!(
                "can't fetch rune outputs: address={} rune={rune} error={:#?}",
                params.address, err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    let mut points: BTreeMap<i64, RuneBalanceHistory> = BTreeMap::new();
    for o in outputs {
        let entry = points.entry(o.block).or_default();
        entry.block = o.block;
        entry.rune_income += o.amount;
        entry.btc_income += o.btc_amount;
        entry.out_count += 1;
    }

    let inputs = match state
        .db
        .select_tx_runes_inputs_sum(&params.address, &rune)
        .await
    {
        Ok(o) => o,
        Err(err) => {
            error!(
                "can't fetch rune inputs: address={} rune={rune} error={:#?}",
                params.address, err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    for i in inputs {
        let entry = points.entry(i.block).or_default();
        entry.block = i.block;
        entry.rune_spent += i.amount;
        entry.btc_spent += i.btc_amount;
        entry.out_count += 1;
    }

    let mut result = points.values().cloned().collect::<Vec<_>>();
    result.sort_by(|a, b| a.block.cmp(&b.block));

    let mut total_btc_balance = 0;
    let mut total_runes_balance = BigDecimal::from(0);
    for p in result.iter_mut() {
        total_btc_balance += p.btc_income;
        total_btc_balance -= p.btc_spent;
        p.btc_balance = total_btc_balance;

        total_runes_balance += &p.rune_income;
        total_runes_balance -= &p.rune_spent;
        p.rune_balance = total_runes_balance.clone();
    }

    Ok(Json(result))
}

pub async fn list_runes_balances(
    state: Data<Context>,
    address: Path<String>,
) -> Result<Json<ListResult<RuneBalance>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let res = state.db.get_runes_balances(&address).await;

    let balances = match res {
        Ok(balances) => balances,
        Err(err) => {
            error!(
                "can't fetch runes balances: address={address} error={:#?}",
                err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    Ok(Json(ListResult {
        records: balances,
        meta: None,
    }))
}

pub async fn list_filtered_runes_balances(
    state: Data<Context>,
    address: Path<String>,
    req: Json<RunesFilter>,
) -> Result<Json<ListResult<RuneBalance>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let res = state.db.get_runes_balances(&address).await;

    let balances = match res {
        Ok(balances) => balances,
        Err(err) => {
            error!(
                "can't fetch runes balances: address={address} error={:#?}",
                err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    let runes: HashSet<String> = req.runes.iter().cloned().collect();
    Ok(Json(ListResult {
        records: balances
            .iter()
            .filter(|&b| runes.contains(&b.rune))
            .cloned()
            .collect::<Vec<RuneBalance>>(),
        meta: None,
    }))
}

pub async fn list_rune_utxos(
    state: Data<Context>,
    params: Path<RuneAddressPath>,
    query: Query<RunesUtxoQuery>,
) -> Result<Json<ListResult<RuneUtxo>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }

    let address = params.address.clone();
    let rune = {
        match ordinals::SpacedRune::from_str(&params.rune) {
            Ok(spr) => spr.rune.to_string(),
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        }
    };

    let (limit, offset) = match query.page.limit_offset() {
        Ok(v) => v,
        Err(err) => {
            return Err(RuneApiError::BadInput(format!("{err}")));
        }
    };

    let count_res = state.db.count_runes_utxo(&rune, &address).await;
    let count = match count_res {
        Ok(c) => c,
        Err(err) => {
            error!("can't count rune utxos: rune={rune} error={:#?}", err);
            0
        }
    };

    let rows_res = state
        .db
        .select_rune_utxo_with_pagination(
            &rune,
            &address,
            query.page.order,
            query.amount_threshold,
            query.sorting,
            limit,
            offset,
        )
        .await;

    let rows = match rows_res {
        Ok(row) => row,
        Err(err) => {
            error!(
                "failed to select runes utxos: rune={} address={} error={:#?}",
                rune, address, err
            );
            return Err(RuneApiError::InternalError);
        }
    };
    let rows = match state.filter_used_runes_utxos(&rows, None).await {
        Ok(r) => r,
        Err(err) => {
            error!(
                "failed to filter runes utxos: address={} error={:#?}",
                params.address, err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    let resp = ListResult {
        meta: Some(ListResponseMeta::new(limit, offset, count as u64)),
        records: rows,
    };

    Ok(Json(resp))
}

pub async fn list_rune_utxos_with_lock(
    state: Data<Context>,
    params: Path<RuneAddressPath>,
    request: Json<CollectRunesUtxo>,
    api_key: super::auth_middleware::XApiKey,
) -> Result<Json<ListResult<RuneUtxo>>, RuneApiError> {
    if !state.is_healthy().await {
        return Err(RuneApiError::ServiceUnavailable);
    }
    let Some(apk) = state.get_api_key(&api_key.0) else {
        return Err(RuneApiError::Unauthorized);
    };
    if let Err(err) = decode_address(&params.address, state.net) {
        return Err(RuneApiError::InvalidAddress(format!("{err}")));
    }

    let target_amount = request.amount.clone();
    let address = params.address.clone();
    let rune = {
        match ordinals::SpacedRune::from_str(&params.rune) {
            Ok(spr) => spr.rune.to_string(),
            Err(err) => {
                return Err(RuneApiError::InvalidRuneName(format!("{err}")));
            }
        }
    };

    if target_amount <= BigDecimal::from(0) || !target_amount.is_integer() {
        return Err(RuneApiError::BadInput(
            "target amount must be positive integer value".into(),
        ));
    }

    let balance = match state.db.get_rune_balance(&address, &rune).await {
        Ok(b) => b,
        Err(err) => {
            error!(
                "can't get rune balance: rune={} address={} err={err:#}",
                rune, address
            );
            return Err(RuneApiError::InternalError);
        }
    };

    if balance.balance < target_amount {
        return Err(RuneApiError::NotEnoughBalance {
            required: target_amount.to_u128().unwrap_or_default(),
            available: balance.balance.to_u128().unwrap(),
        });
    }

    let collected = coolect_utxo_shortcut(
        &state,
        &rune,
        &address,
        apk.can_lock_utxo,
        target_amount.clone(),
        &request.request_id,
    )
    .await?;
    if let Some(resp) = collected {
        return Ok(Json(resp));
    }

    let mut collected_utxos = Vec::new();
    let limit = 200;
    let mut offset = 0;

    loop {
        let rows_res = state
            .db
            .select_rune_utxo_with_pagination(
                &rune,
                &address,
                OrderBy::Desc,
                None,
                UtxoSortMode::Amount,
                limit,
                offset,
            )
            .await;

        let rows = match rows_res {
            Ok(row) => row,
            Err(err) => {
                error!(
                    "failed to select runes utxos: rune={} address={} error={:#?}",
                    rune, address, err
                );
                return Err(RuneApiError::InternalError);
            }
        };
        if rows.is_empty() {
            return Err(RuneApiError::NotEnoughBalance {
                required: target_amount.to_u128().unwrap_or_default(),
                available: collected_utxos
                    .iter()
                    .map(|e: &RuneUtxo| e.amount.to_u128().unwrap_or_default())
                    .sum::<u128>(),
            });
        }
        match state
            .filter_used_runes_utxos(&rows, Some(request.request_id.clone()))
            .await
        {
            Ok(r) => collected_utxos.extend(r),
            Err(err) => {
                error!(
                    "failed to filter runes utxos: address={} error={:#?}",
                    params.address, err
                );
                return Err(RuneApiError::InternalError);
            }
        };

        let result = min_utxos_to_reach_target(
            &collected_utxos,
            request.amount.to_u128().unwrap_or_default(),
        );
        match result {
            Ok(utxos) => {
                lock_utxo(&state, apk.can_lock_utxo, &utxos, &request.request_id).await;
                let resp = ListResult {
                    meta: Some(ListResponseMeta::new(limit, offset, utxos.len() as u64)),
                    records: utxos,
                };

                return Ok(Json(resp));
            }
            Err(KnapsackError::NotEnoughBalance { .. }) => {
                // try to select more utxos
                offset += limit;
                continue;
            }
        }
    }
}

async fn coolect_utxo_shortcut(
    state: &Context,
    rune: &str,
    address: &str,
    can_lock_utxo: bool,
    target_amount: BigDecimal,
    rid: &str,
) -> Result<Option<ListResult<RuneUtxo>>, RuneApiError> {
    let lower_bound = &target_amount / BigDecimal::from(10);
    let upper_bound = &target_amount * BigDecimal::from(4);
    // shortcut
    let rows_res = state
        .db
        .select_rune_utxos_with_amount_bounds(rune, address, 10, &lower_bound, &upper_bound)
        .await;
    let rows = match rows_res {
        Ok(row) => row,
        Err(err) => {
            error!(
                "failed to select runes utxos: rune={} address={} error={:#?}",
                rune, address, err
            );
            return Err(RuneApiError::InternalError);
        }
    };
    let target_amount = target_amount
        .to_u128()
        .expect("rune amount is out of bounds");

    // filter & collect
    let rows = match state.filter_used_runes_utxos(&rows, Some(rid.into())).await {
        Ok(r) => r,
        Err(err) => {
            error!(
                "failed to filter runes utxos: address={} error={:#?}",
                address, err
            );
            return Err(RuneApiError::InternalError);
        }
    };

    let result = min_utxos_to_reach_target(&rows, target_amount);
    match result {
        Ok(utxos) => {
            lock_utxo(state, can_lock_utxo, &utxos, rid).await;
            let resp = ListResult {
                meta: Some(ListResponseMeta::new(10, 0, rows.len() as u64)),
                records: utxos,
            };

            Ok(Some(resp))
        }
        Err(KnapsackError::NotEnoughBalance { .. }) => {
            debug!("shortcut is unsuccessful, going in hard way");
            Ok(None)
        }
    }
}

async fn lock_utxo(state: &Context, can_lock_utxo: bool, utxos: &[RuneUtxo], rid: &str) {
    let Some(cache) = state.cache.as_ref() else {
        return;
    };
    if !can_lock_utxo {
        return;
    }

    for u in utxos.iter() {
        let res = cache.lock_utxo(&u.tx_hash, u.vout, rid).await;
        if let Err(err) = res {
            error!("unable to write utxo lock: id={rid} error={err:#}");
        }
    }
}

pub async fn get_tx_runes_utxos(
    state: Data<Context>,
    txid: Path<types::Hash>,
) -> Result<Json<RuneTxInOuts>, FBtcApiError> {
    // TODO (?)
    // 3. calculate fee
    // 4. (? block info )

    let outputs = match state.db.select_tx_runes_outputs(&txid).await {
        Ok(outs) => outs,
        Err(err) => {
            error!("select of tx outs failed: tx={} err={:#}", txid, err);
            return Err(FBtcApiError::InternalError);
        }
    };

    let inputs = match state.db.select_tx_runes_inputs_ext(&txid).await {
        Ok(inputs) => inputs,
        Err(err) => {
            error!("select of tx inputs failed: tx={} err={:#}", txid, err);
            return Err(FBtcApiError::InternalError);
        }
    };

    Ok(Json(RuneTxInOuts { inputs, outputs }))
}
