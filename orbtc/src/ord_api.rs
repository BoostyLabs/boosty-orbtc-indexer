use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub struct OrdClient {
    address: String,
    client: reqwest::Client,
}

impl OrdClient {
    pub fn new(address: &str) -> Self {
        let client = reqwest::Client::new();

        Self {
            address: address.to_owned(),
            client,
        }
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: impl Serialize,
    ) -> anyhow::Result<T> {
        let url = format!("{}{}", self.address, path);
        let result = self
            .client
            .post(&url)
            .json(&body)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("attempt to GET {} failed", url))?
            .json::<T>()
            .await
            .with_context(|| format!("unable to parse json response from {}", url))?;

        Ok(result)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = format!("{}{}", self.address, path);
        let result = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .with_context(|| format!("attempt to GET {} failed", url))?
            .json::<T>()
            .await
            .with_context(|| format!("unable to parse json response from {}", url))?;

        Ok(result)
    }

    pub async fn get_details(&self, outpoints: &[String]) -> anyhow::Result<Vec<OutputInfo>> {
        self.post("/outputs", outpoints).await
    }

    pub async fn get_inscription(&self, id: u64) -> anyhow::Result<Option<Inscription>> {
        self.get(format!("/inscription/{id}").as_str()).await
    }
}

#[derive(Clone)]
pub struct OrdClientSync {
    address: String,
    client: reqwest::blocking::Client,
}

impl OrdClientSync {
    pub fn new(address: &str) -> Self {
        let client = reqwest::blocking::Client::new();

        Self {
            address: address.to_owned(),
            client,
        }
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: impl Serialize) -> anyhow::Result<T> {
        let url = format!("{}{}", self.address, path);
        let result = self
            .client
            .post(&url)
            .json(&body)
            .header("Accept", "application/json")
            .send()
            .with_context(|| format!("attempt to POST {} failed", url))?
            .json::<T>()
            .with_context(|| format!("unable to parse json response from {}", url))?;

        Ok(result)
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = format!("{}{}", self.address, path);
        let result = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .send()
            .with_context(|| format!("attempt to GET {} failed", url))?
            .json::<T>()
            .with_context(|| format!("unable to parse json response from {}", url))?;

        Ok(result)
    }

    pub fn get_details(&self, outpoints: &[String]) -> anyhow::Result<Vec<OutputInfo>> {
        self.post("/outputs", outpoints)
    }

    pub fn output_details(&self, outpoint: &str) -> anyhow::Result<OutputInfo> {
        self.get(format!("/output/{}", outpoint).as_str())
    }

    pub fn get_inscription(&self, id: u64) -> anyhow::Result<Option<Inscription>> {
        self.get(format!("/inscription/{id}").as_str())
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inscription {
    // pub id: String,
    // pub number: u64,
    pub satpoint: String,
}

impl Inscription {
    pub fn output(&self) -> Option<(String, i32)> {
        let mut parts = self.satpoint.split(":");
        let txid = parts.next()?;
        let vout = parts.next()?;
        let vout: i32 = vout.parse().ok()?;

        Some((txid.to_owned(), vout))
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub inscriptions: Vec<String>,
    pub outpoint: String,
    // pub address: String,
    // pub confirmations: i64,
    // pub indexed: bool,
    pub runes: HashMap<String, Rune>,
    // pub sat_ranges: Vec<Vec<i64>>,
    // pub script_pubkey: String,
    // pub spent: bool,
    // pub transaction: String,
    // pub value: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rune {
    // pub amount: i64,
    // pub divisibility: i64,
    pub symbol: String,
}
