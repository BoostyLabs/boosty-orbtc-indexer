use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::str::FromStr;

use actix_web::web::{self, Data, Json, Path, Query};
use api_core::pages::{ListResponseMeta, ListResult};
use bitcoincore_rpc::RpcApi;
use orbtc_indexer_api::btc::*;
use orbtc_indexer_api::{types, OrderBy, UtxoSortMode};
use serde::Deserialize;

use super::context::Context;
use super::requests::{decode_address, FeeRate};
use crate::service::utxo_collector::{min_utxos_to_reach_target, KnapsackError};

#[derive(Deserialize)]
pub struct GetBalanceParams {
    pub address: String,
}

pub async fn get_balance(
    state: Data<Context>,
    params: Path<GetBalanceParams>,
) -> Result<Json<Balance>, FBtcApiError> {
    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }

    if let Err(err) = decode_address(&params.address, state.net) {
        return Err(FBtcApiError::InvalidAddress(format!("{err}")));
    }

    match state.db.get_balance(&params.address).await {
        Ok(balance) => Ok(Json(balance)),

        Err(err) => {
            error!(
                "can't fetch btc balance: address={} error={:#?}",
                params.address, err
            );
            Err(FBtcApiError::InternalError)
        }
    }
}

pub async fn get_balance_history(
    state: Data<Context>,
    params: Path<GetBalanceParams>,
) -> Result<Json<Vec<BtcBalanceHistoryPoint>>, FBtcApiError> {
    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }

    if let Err(err) = decode_address(&params.address, state.net) {
        return Err(FBtcApiError::InvalidAddress(format!("{err}")));
    }
    let outputs = match state.db.select_tx_outputs_sum(&params.address).await {
        Ok(o) => o,
        Err(err) => {
            error!(
                "can't fetch btc outputs: address={} error={:#?}",
                params.address, err
            );
            return Err(FBtcApiError::InternalError);
        }
    };

    let mut points: BTreeMap<i64, BtcBalanceHistoryPoint> = BTreeMap::new();
    for o in outputs {
        let entry = points.entry(o.block).or_default();
        entry.block = o.block;
        entry.balance += o.amount;
        entry.income += o.amount;
        entry.out_count += 1;
    }

    let inputs = match state.db.select_tx_inputs_sum(&params.address).await {
        Ok(o) => o,
        Err(err) => {
            error!(
                "can't fetch btc inputs: address={} error={:#?}",
                params.address, err
            );
            return Err(FBtcApiError::InternalError);
        }
    };

    for i in inputs {
        let entry = points.entry(i.block).or_default();
        entry.block = i.block;
        entry.balance -= i.amount;
        entry.spent += i.amount;
        entry.out_count += 1;
    }

    let mut result = points.values().cloned().collect::<Vec<_>>();
    result.sort_by(|a, b| a.block.cmp(&b.block));
    let mut total_btc_balance = 0;
    for p in result.iter_mut() {
        total_btc_balance += p.income;
        total_btc_balance -= p.spent;
        p.balance = total_btc_balance;
    }

    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct UtxoRequest {
    pub address: String,
}

pub async fn list_utxos(
    state: Data<Context>,
    params: Path<UtxoRequest>,
    query: Query<UtxoQuery>,
) -> Result<Json<ListResult<BtcUtxo>>, FBtcApiError> {
    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }

    if let Err(err) = decode_address(&params.address, state.net) {
        return Err(FBtcApiError::InvalidAddress(format!("{err}")));
    }
    let (limit, offset) = match query.page.limit_offset() {
        Ok(v) => v,
        Err(err) => {
            return Err(FBtcApiError::BadInput(format!("{err}")));
        }
    };

    let count = match state.db.count_utxos(&params.address).await {
        Ok(c) => c,
        Err(err) => {
            error!("can't count utxos: error={:#?}", err);
            0
        }
    };

    #[rustfmt::skip]
    let older_than = if query.skip_premature {
         match state.btc_client.get_block_count() {
            Ok(block) => if block > 100 { Some(block - 100) } else { None },
            Err(_) => None,
        }
    } else { None };

    let mut db_limit = limit;
    let mut db_offset = offset;
    let mut records = Vec::new();
    'collector: loop {
        let rows_res = state
            .db
            .select_utxo_with_pagination(
                &params.address,
                query.page.order,
                query.amount_threshold,
                older_than,
                query.sorting,
                db_limit,
                db_offset,
            )
            .await;
        let row = match rows_res {
            Ok(row) => row,
            Err(err) => {
                error!(
                    "failed to select btc utxos: address={} error={:#?}",
                    params.address, err
                );
                return Err(FBtcApiError::InternalError);
            }
        };
        if row.is_empty() {
            break 'collector;
        }

        let rows = match state
            .filter_used_btc_utxos(&row, query.no_runes, None)
            .await
        {
            Ok(r) => r,
            Err(err) => {
                error!(
                    "failed to filter runes utxos: address={} error={:#?}",
                    params.address, err
                );
                return Err(FBtcApiError::InternalError);
            }
        };

        records.extend(rows);
        if records.len() as u32 >= limit {
            break 'collector;
        }

        db_offset += db_limit;
        db_limit = limit - records.len() as u32;
    }

    let resp = ListResult {
        meta: Some(ListResponseMeta {
            page: query.page.page.unwrap_or_default(),
            limit,
            offset: db_offset,
            total_records: count as u64,
            has_more: i64::from(db_offset + limit) < count,
        }),
        records,
    };

    Ok(Json(resp))
}

pub async fn list_utxos_with_lock(
    state: Data<Context>,
    params: Path<UtxoRequest>,
    request: Json<CollectUtxo>,
    api_key: super::auth_middleware::XApiKey,
) -> Result<Json<ListResult<BtcUtxo>>, FBtcApiError> {
    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }
    let Some(apk) = state.get_api_key(&api_key.0) else {
        return Err(FBtcApiError::Unauthorized);
    };
    if let Err(err) = decode_address(&params.address, state.net) {
        return Err(FBtcApiError::InvalidAddress(format!("{err}")));
    }

    let target_amount = request.amount;
    let address = params.address.clone();

    if target_amount == 0 {
        return Err(FBtcApiError::BadInput(
            "target amount must be positive integer value".into(),
        ));
    }

    let balance = match state.db.get_balance(&address).await {
        Ok(b) => b,
        Err(err) => {
            error!("can't get balance: address={} err={err:#}", address);
            return Err(FBtcApiError::InternalError);
        }
    };

    if (balance.balance as u64) < target_amount {
        return Err(FBtcApiError::NotEnoughBalance {
            required: target_amount as u128,
            available: balance.balance as u128,
        });
    }

    #[rustfmt::skip]
    let older_than = match state.btc_client.get_block_count() {
        Ok(block) => if block > 100 { Some(block - 100) } else { None },
        Err(_) => None,
    };

    let collected = coolect_utxo_shortcut(
        &state,
        &address,
        apk.can_lock_utxo,
        target_amount,
        &request.request_id,
        older_than.unwrap_or_default(),
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
            .select_utxo_with_pagination(
                &address,
                OrderBy::Desc,
                None,
                older_than,
                UtxoSortMode::Amount,
                limit,
                offset,
            )
            .await;

        let rows = match rows_res {
            Ok(row) => row,
            Err(err) => {
                error!(
                    "failed to select btc utxos: address={} error={:#?}",
                    address, err
                );
                return Err(FBtcApiError::InternalError);
            }
        };
        if rows.is_empty() {
            return Err(FBtcApiError::NotEnoughBalance {
                required: target_amount as u128,
                available: collected_utxos
                    .iter()
                    .map(|e: &BtcUtxo| e.amount as u128)
                    .sum::<u128>(),
            });
        }
        match state
            .filter_used_btc_utxos(&rows, true, Some(request.request_id.clone()))
            .await
        {
            Ok(r) => collected_utxos.extend(r),
            Err(err) => {
                error!(
                    "failed to filter btc utxos: address={} error={:#?}",
                    params.address, err
                );
                return Err(FBtcApiError::InternalError);
            }
        };

        let result = min_utxos_to_reach_target(&collected_utxos, request.amount as u128);
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
    address: &str,
    can_lock_utxo: bool,
    target_amount: u64,
    rid: &str,
    older_than: u64,
) -> Result<Option<ListResult<BtcUtxo>>, FBtcApiError> {
    let lower_bound = target_amount / 10;
    let upper_bound = target_amount * 4;
    // shortcut
    let rows_res = state
        .db
        .select_utxos_with_amount_bounds(address, 10, lower_bound, upper_bound, older_than)
        .await;
    let rows = match rows_res {
        Ok(row) => row,
        Err(err) => {
            error!(
                "failed to select btc utxos: address={} error={:#?}",
                address, err
            );
            return Err(FBtcApiError::InternalError);
        }
    };

    // filter & collect
    let rows = match state
        .filter_used_btc_utxos(&rows, true, Some(rid.into()))
        .await
    {
        Ok(r) => r,
        Err(err) => {
            error!(
                "failed to filter runes utxos: address={} error={:#?}",
                address, err
            );
            return Err(FBtcApiError::InternalError);
        }
    };

    let result = min_utxos_to_reach_target(&rows, target_amount.into());
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

async fn lock_utxo(state: &Context, can_lock_utxo: bool, utxos: &[BtcUtxo], rid: &str) {
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

pub async fn btc_fee_rate(state: Data<Context>) -> Result<Json<FeeRate>, FBtcApiError> {
    match state.estimate_fee().await {
        Ok(mut fee) => {
            use std::cmp::max;
            let min_rate = state.cfg.min_fee_rate;
            fee.min = max(min_rate, fee.min);
            fee.normal = max(min_rate, fee.normal);
            fee.fast = max(min_rate, fee.fast);

            Ok(Json(fee))
        }
        Err(err) => {
            error!("unable to estimage fee: error={:#}", err);

            Err(FBtcApiError::InternalError)
        }
    }
}

pub async fn get_transaction(
    state: Data<Context>,
    txid: web::Path<String>,
) -> Result<Json<GetTxResponse>, FBtcApiError> {
    use bitcoincore_rpc::jsonrpc::Error::Rpc as BtcRpcError;
    use bitcoincore_rpc::Error::JsonRpc as BtcJsonRpcError;

    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }

    let txid = match bitcoincore_rpc::bitcoin::Txid::from_str(&txid) {
        Ok(txid) => txid,
        Err(err) => {
            warn!("invalid txid={txid} error={err}");
            return Err(FBtcApiError::BadInput(format!("invalid txid={txid}")));
        }
    };

    let txinfo = match state.btc_client.get_raw_transaction_info(&txid, None) {
        // we got a response from bitcoind
        Ok(response) => response,

        // we got jsonrpc error, we want to return it as an error "value"
        Err(BtcJsonRpcError(BtcRpcError(ref rpc_error))) => {
            error!("get_raw_transaction_info jsonrpc error: {:#?}", rpc_error);
            return Ok(Json(GetTxResponse {
                result: None,
                error: Some(RpcError {
                    code: rpc_error.code,
                    message: rpc_error.message.clone(),
                    data: rpc_error.data.clone(),
                }),
            }));
        }

        // we got non-jsonrpc error. Treat them as internal errors.
        Err(e) => {
            error!("get_raw_transaction_info internal error: {:#?}", e);
            return Err(FBtcApiError::InternalError);
        }
    };

    // if the tx is in a block, we can get a block height and a tx number (position in a block), and return that.
    let (blockheight, txnumber) = if let Some(blockhash) = txinfo.blockhash {
        // transaction is in a block
        let blockinfo = match state.btc_client.get_block_info(&blockhash) {
            Ok(blockinfo) => blockinfo,
            Err(e) => {
                error!("getrawtransaction returned that tx={} is in a block={}, but cannot get block info. error={:#?}", txid, blockhash, e);
                return Err(FBtcApiError::InternalError);
            }
        };

        let blockheight = blockinfo.height;
        let txnumber = blockinfo
            .tx
            .iter()
            .position(|blocktxid| blocktxid == &txid)
            .expect("if blockhash is set, txid must be in a block");

        (Some(blockheight), Some(txnumber))
    } else {
        (None, None)
    };

    Ok(Json(GetTxResponse {
        result: Some(RawTxInfo {
            in_active_chain: txinfo.in_active_chain,
            confirmations: txinfo.confirmations,
            time: txinfo.time,
            blocktime: txinfo.blocktime,
            blockhash: txinfo.blockhash.map(|h| h.to_string()),
            blockheight,
            txnumber,
            raw_tx: hex::encode(txinfo.hex),
        }),
        error: None,
    }))
}

pub async fn send_raw_transaction(
    state: Data<Context>,
    req: web::Json<SendTxRequest>,
) -> Result<Json<SendTxResponse>, FBtcApiError> {
    use bitcoincore_rpc::jsonrpc::Error::Rpc as BtcRpcError;
    use bitcoincore_rpc::Error::JsonRpc as BtcJsonRpcError;

    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }

    let tx = req.tx.clone();
    let txid = match state.btc_client.send_raw_transaction(tx) {
        // successfully submitted tx to a mempool
        Ok(txid) => txid.to_string(),

        // we got some jsonrpc error
        Err(BtcJsonRpcError(BtcRpcError(ref rpc_error))) => {
            warn!("send_raw_transaction jsonrpc error: {:#?}", rpc_error);

            return Ok(Json(SendTxResponse {
                result: None,
                error: Some(RpcError {
                    code: rpc_error.code,
                    message: rpc_error.message.clone(),
                    data: rpc_error.data.clone(),
                }),
            }));
        }

        // we got non-jsonrpc error. Treat them as internal errors.
        Err(e) => {
            error!("send_raw_transaction internal error: {:#?}", e);
            return Err(FBtcApiError::InternalError);
        }
    };

    debug!("successfully sent tx={txid} to a mempool");
    Ok(Json(SendTxResponse {
        result: Some(TxHash {
            tx_hash: txid.to_string(),
        }),
        error: None,
    }))
}

pub async fn get_txs_in_mempool(
    state: Data<Context>,
) -> Result<Json<GetMempoolTxsResponse>, FBtcApiError> {
    use bitcoincore_rpc::jsonrpc::Error::Rpc as BtcRpcError;
    use bitcoincore_rpc::Error::JsonRpc as BtcJsonRpcError;

    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }
    let result = match state.btc_client.get_raw_mempool() {
        // we got a response from bitcoind
        Ok(response) => response,

        // we got jsonrpc error, we want to return it as an error "value"
        Err(BtcJsonRpcError(BtcRpcError(ref rpc_error))) => {
            error!("get_raw_mempool jsonrpc error: {:#?}", rpc_error);
            return Ok(Json(GetMempoolTxsResponse {
                result: None,
                error: Some(RpcError {
                    code: rpc_error.code,
                    message: rpc_error.message.clone(),
                    data: rpc_error.data.clone(),
                }),
            }));
        }

        // we got non-jsonrpc error. Treat them as internal errors.
        Err(e) => {
            error!("get_raw_transaction_info internal error: {:#?}", e);
            return Err(FBtcApiError::InternalError);
        }
    };

    Ok(Json(GetMempoolTxsResponse {
        result: Some(result),
        error: None,
    }))
}

pub async fn get_tx_in_outs(
    state: Data<Context>,
    txid: web::Path<types::Hash>,
) -> Result<Json<TxInOuts>, FBtcApiError> {
    // TODO (?)
    // 3. calculate fee
    // 4. (? block info )

    let outputs = match state.db.select_tx_outputs(&txid).await {
        Ok(outs) => outs,
        Err(err) => {
            error!("select of tx outs failed: tx={} err={:#}", txid, err);
            return Err(FBtcApiError::InternalError);
        }
    };
    let inputs = match state.db.select_tx_inputs_ext(&txid).await {
        Ok(inputs) => inputs,
        Err(err) => {
            error!("select of tx inputs failed: tx={} err={:#}", txid, err);
            return Err(FBtcApiError::InternalError);
        }
    };

    Ok(Json(TxInOuts { inputs, outputs }))
}

pub async fn list_address_txs(
    state: Data<Context>,
    params: Path<UtxoRequest>,
    query: Query<ListTxQuery>,
) -> Result<Json<ListResult<TxInfo>>, FBtcApiError> {
    if !state.is_healthy().await {
        return Err(FBtcApiError::ServiceUnavailable);
    }

    if let Err(err) = decode_address(&params.address, state.net) {
        return Err(FBtcApiError::InvalidAddress(format!("{err}")));
    }
    let (limit, offset) = match query.page.limit_offset() {
        Ok(v) => v,
        Err(err) => {
            return Err(FBtcApiError::BadInput(format!("{err}")));
        }
    };

    let income_res = state
        .db
        .list_address_incoming_txs(&params.address, query.page.order, limit, offset)
        .await;
    let income_rows = match income_res {
        Ok(row) => row,
        Err(err) => {
            error!(
                "failed to select btc utxos: address={} error={:#?}",
                params.address, err
            );
            return Err(FBtcApiError::InternalError);
        }
    };

    if query.incoming_only {
        let records: Vec<TxInfo> = income_rows
            .iter()
            .map(|e| TxInfo {
                tx_hash: e.tx_hash.clone(),
                block: e.block,
                spend: false,
                income: true,
            })
            .collect();
        let resp = ListResult {
            meta: Some(ListResponseMeta {
                page: query.page.page.unwrap_or_default(),
                limit,
                offset,
                total_records: 0,
                has_more: true,
            }),
            records,
        };

        return Ok(Json(resp));
    }

    let spend_res = state
        .db
        .list_address_outgoing_txs(&params.address, query.page.order, limit, offset)
        .await;
    let spend_rows = match spend_res {
        Ok(row) => row,
        Err(err) => {
            error!(
                "failed to select btc utxos: address={} error={:#?}",
                params.address, err
            );
            return Err(FBtcApiError::InternalError);
        }
    };

    let income_idx: BTreeSet<_> = income_rows.iter().map(|e| e.tx_hash.clone()).collect();
    let out_idx: BTreeSet<_> = spend_rows.iter().map(|e| e.tx_hash.clone()).collect();

    let mut records: Vec<TxInfo> = spend_rows
        .iter()
        .map(|e| TxInfo {
            tx_hash: e.tx_hash.clone(),
            block: e.block,
            spend: true,
            income: income_idx.contains(&e.tx_hash),
        })
        .collect();

    let extra: Vec<_> = spend_rows
        .iter()
        .filter_map(|e| {
            if out_idx.contains(&e.tx_hash) {
                None
            } else {
                Some(TxInfo {
                    tx_hash: e.tx_hash.clone(),
                    block: e.block,
                    spend: false,
                    income: true,
                })
            }
        })
        .collect();
    records.extend(extra);

    let resp = ListResult {
        meta: Some(ListResponseMeta {
            page: query.page.page.unwrap_or_default(),
            limit,
            offset,
            total_records: 0,
            has_more: true,
        }),
        records,
    };

    Ok(Json(resp))
}
