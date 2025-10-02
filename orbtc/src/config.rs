use std::fs;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub static CONFIG: OnceLock<Config> = OnceLock::new();

pub fn get() -> &'static Config {
    CONFIG.get().expect("config already set")
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub api: api_core::server::Config,
    pub btc: BTCConfig,
    pub db: DBConfig,
    #[serde(default = "defaults::fee_adjustment")]
    pub fee_adjustement: u64,
    #[serde(default = "defaults::min_fee_rate")]
    pub min_fee_rate: u64,
    #[serde(default)]
    pub metrics: api_core::server::MetricsConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    #[serde(default)]
    pub ord_api: OrdConfig,
    #[serde(default)]
    pub firehose_api_key: Option<String>,
}

impl Config {
    pub fn read(path: &str) -> anyhow::Result<Config> {
        let contents = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;

        let _ = CONFIG.set(config.clone());

        Ok(config)
    }

    pub fn get_api_url(&self) -> String {
        format!("http://{}:{}", self.api.listen_address, self.api.port)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DBConfig {
    pub dsn: String,
    pub automigrate: bool,
    #[serde(default)]
    pub force_migration: bool,
}

impl Default for DBConfig {
    fn default() -> Self {
        DBConfig {
            dsn: "postgres://postgres:postgres@localhost:5432/btc_indexer".to_string(),
            automigrate: true,
            force_migration: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BTCConfig {
    pub network: Option<String>,
    pub address: String,
    pub rpc_user: String,
    pub rpc_password: String,
}

impl Default for BTCConfig {
    fn default() -> Self {
        Self {
            network: Some("mainnet".to_string()),
            address: "127.0.0.1:8443".to_string(),
            rpc_user: "".to_string(),
            rpc_password: "".to_string(),
        }
    }
}

impl BTCConfig {
    pub fn get_network(&self) -> bitcoin::Network {
        let Some(net) = self.network.clone() else {
            return bitcoin::Network::Bitcoin;
        };

        match net.as_str() {
            "mainnet" => bitcoin::Network::Bitcoin,
            "testnet" => bitcoin::Network::Testnet,
            "testnet4" => bitcoin::Network::Testnet4,
            "regtest" => bitcoin::Network::Regtest,
            "signet" => bitcoin::Network::Signet,
            _ => bitcoin::Network::Bitcoin,
        }
    }
}

#[derive(Default, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub enable: bool,
    #[serde(default)]
    pub redis: String,
    #[serde(default)]
    pub lock_ttl: u64,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct OrdConfig {
    #[serde(default)]
    pub address: Option<String>,
}

mod defaults {
    pub fn fee_adjustment() -> u64 {
        0
    }
    pub fn min_fee_rate() -> u64 {
        1
    }
}
