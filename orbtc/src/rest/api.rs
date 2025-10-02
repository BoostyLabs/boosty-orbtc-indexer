use actix_web::middleware::from_fn;
use actix_web::web::{get, post, resource, scope, Data, Json};
use actix_web::{HttpResponse, Responder, Scope};
use api_core::server::APIProvider;
use bitcoin::Network;
use orbtc_indexer_api::StatusResponse;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::api_btc::*;
use super::api_runes::*;
use super::auth_middleware::ensure_api_key;
use super::context::{update_metrics, Context};
use super::{mempool_cache, swagger};

#[derive(Clone)]
pub struct Service {
    pub context: Context,
}

impl Service {
    pub async fn new(cfg: crate::config::Config) -> anyhow::Result<Self> {
        let context = Context::new(cfg).await?;
        Ok(Self { context })
    }

    pub fn spawn_jobs(&self, cancel: CancellationToken) {
        // This task don't have any persistent state.
        // So, we don't care about gracefull shutdown of this task.
        tokio::spawn(mempool_cache::refresh_state_routine(
            self.context.mempool_index.clone(),
            cancel.clone(),
        ));
        tokio::spawn(update_metrics(
            self.context.metrics_collector.clone(),
            cancel,
        ));
    }
}

const fn net_as_str(net: Network) -> &'static str {
    match net {
        Network::Bitcoin => "mainnet",
        Network::Testnet4 => "testnet4",
        Network::Signet => "signet",
        Network::Regtest => "regtest",
        _ => "mainnet",
    }
}

impl APIProvider for Service {
    fn name(&self) -> &'static str {
        "orbtc_api"
    }

    fn service(&self) -> Scope {
        let net = net_as_str(self.context.net);
        info!("Preparing API SCOPES: {net}");

        scope("/v1")
            .app_data(Data::new(self.context.clone()))
            .service(resource("/healthcheck").route(get().to(healthcheck)))
            .service(resource("/version").route(get().to(version)))
            .service(resource("/swagger").route(get().to(swagger::ui)))
            .service(resource("/swagger/swagger.yaml").route(get().to(swagger::spec)))
            .service(
                scope(&format!("/{}", net))
                    .wrap(from_fn(ensure_api_key))
                    .service(resource("/status").route(get().to(service_status)))
                    .service(
                        resource("/utxos/{address}")
                            .route(get().to(list_utxos))
                            .route(post().to(list_utxos_with_lock)),
                    )
                    .service(resource("/balance/{address}").route(get().to(get_balance)))
                    .service(
                        resource("/balance-history/{address}").route(get().to(get_balance_history)),
                    )
                    .service(resource("/balance/{address}").route(get().to(get_balance)))
                    .service(resource("/fee-rate").route(get().to(btc_fee_rate)))
                    .service(resource("/runes").route(get().to(list_runes)))
                    .service(resource("/runes/search").route(get().to(list_runes)))
                    .service(resource("/runes/{rune}").route(get().to(get_rune)))
                    .service(
                        resource("/runes/{rune}/utxos/{address}")
                            .route(get().to(list_rune_utxos))
                            .route(post().to(list_rune_utxos_with_lock)),
                    )
                    .service(resource("/runes/{rune}/balance").route(get().to(list_rune_holders)))
                    .service(
                        resource("/runes/{rune}/balance/{address}")
                            .route(get().to(get_rune_balance)),
                    )
                    .service(
                        resource("/runes/{rune}/balance-history/{address}")
                            .route(get().to(get_rune_balance_history)),
                    )
                    .service(
                        resource("/runes/balance/{address}")
                            .route(get().to(list_runes_balances))
                            .route(post().to(list_filtered_runes_balances)),
                    )
                    .service(resource("/txs/address/{address}").route(get().to(list_address_txs)))
                    .service(resource("/tx").route(post().to(send_raw_transaction)))
                    .service(resource("/tx/{txid}").route(get().to(get_transaction)))
                    .service(resource("/tx/{txid}/ins-outs").route(get().to(get_tx_in_outs)))
                    .service(
                        resource("/tx/{txid}/ins-outs/runes").route(get().to(get_tx_runes_utxos)),
                    )
                    .service(resource("/mempool/tx-list").route(get().to(get_txs_in_mempool))),
            )
    }
}

async fn healthcheck(state: Data<Context>) -> impl Responder {
    let status = state.metrics_collector.service_status().await;
    if status.healthy {
        HttpResponse::Ok().finish()
    } else {
        HttpResponse::ServiceUnavailable().finish()
    }
}

async fn version() -> impl Responder {
    let info = get_app_info();
    HttpResponse::Ok().json(info)
}

async fn service_status(state: Data<Context>) -> Json<StatusResponse> {
    let status = state.metrics_collector.service_status().await;
    Json(status)
}

#[derive(Serialize)]
pub struct AppInfo {
    pub app: &'static str,
    pub version: &'static str,
    pub build: &'static str,
    pub commit: &'static str,
}

fn get_app_info() -> AppInfo {
    const APP: &str = env!("CARGO_CRATE_NAME");
    const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");

    #[inline]
    fn git_version() -> &'static str {
        option_env!("GIT_VERSION").unwrap_or("n/a")
    }

    #[inline]
    fn git_commit() -> &'static str {
        option_env!("GIT_COMMIT").unwrap_or("n/a")
    }

    AppInfo {
        app: APP,
        version: PKG_VERSION,
        build: git_version(),
        commit: git_commit(),
    }
}
