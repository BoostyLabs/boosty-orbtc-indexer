use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::rest::requests::FeeRate;

#[derive(Clone)]
pub struct MempoolClient {
    net: bitcoin::Network,
    client: reqwest::Client,
    fee_adjustment: u64,
}

impl MempoolClient {
    pub fn new(net: bitcoin::Network, fee_adjustment: u64) -> Self {
        let client = reqwest::Client::new();

        Self {
            net,
            client,
            fee_adjustment,
        }
    }

    async fn request<T: DeserializeOwned>(&self, url: &str) -> anyhow::Result<T> {
        let result = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| format!("attempt to GET {} failed", url))?
            .json::<T>()
            .await
            .with_context(|| format!("unable to parse json response from {}", url))?;

        Ok(result)
    }

    pub async fn get_fee(&self) -> anyhow::Result<FeeRate> {
        let req = match self.net {
            bitcoin::Network::Testnet => "https://mempool.space/testnet/api/v1/fees/recommended",
            bitcoin::Network::Testnet4 => "https://mempool.space/testnet4/api/v1/fees/recommended",
            bitcoin::Network::Bitcoin => "https://mempool.space/api/v1/fees/recommended",
            _ => {
                anyhow::bail!("network({}) is not supported", self.net)
            }
        };

        let fee_response = self.request::<FeesRecommended>(req).await?;

        let fee = FeeRate {
            fast: adjust_fee(fee_response.fastest_fee, self.fee_adjustment),
            normal: adjust_fee(fee_response.half_hour_fee, self.fee_adjustment),
            min: adjust_fee(fee_response.hour_fee, self.fee_adjustment),
        };

        Ok(fee)
    }
}

fn adjust_fee(val: u64, adjustment: u64) -> u64 {
    val + (val * adjustment / 10)
}

// {"fastestFee":81,"halfHourFee":65,"hourFee":56,"economyFee":2,"minimumFee":1}
#[derive(Copy, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FeesRecommended {
    fastest_fee: u64,
    half_hour_fee: u64,
    hour_fee: u64,
    economy_fee: u64,
    minimum_fee: u64,
}
