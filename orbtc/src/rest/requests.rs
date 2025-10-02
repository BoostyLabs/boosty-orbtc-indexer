use std::str::FromStr;

use bitcoin::address::NetworkChecked;
use bitcoin::{Address, Network};
use serde::{Deserialize, Serialize};

pub fn decode_address(address: &str, net: Network) -> anyhow::Result<Address<NetworkChecked>> {
    Ok(Address::from_str(address)?.require_network(net)?)
}

#[derive(Copy, Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeeRate {
    pub fast: u64,
    pub normal: u64,
    pub min: u64,
}
