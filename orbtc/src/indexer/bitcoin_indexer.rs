use bitcoin::Address;
use orbtc_indexer_api::types::Hash;

use super::bitcoin_indexer_state::StateProvider;
use super::db;
use super::rt::{TxIndexer, TxInfo};
use crate::config;
use crate::db::schema;

// do not change this value. If you do, modify migration!
pub const BITCOIN_INDEX: &str = "btc_utxo_index";

pub struct BitcoinUtxoIndexer {
    net: bitcoin::Network,

    state: StateProvider,
}

impl BitcoinUtxoIndexer {
    pub fn new(net: bitcoin::Network, db_cfg: &config::DBConfig) -> Self {
        let db = db::DB::establish_connection(&db_cfg.dsn);
        Self {
            net,
            state: StateProvider::new(db),
        }
    }
}

impl TxIndexer for BitcoinUtxoIndexer {
    fn name(&self) -> String {
        BITCOIN_INDEX.into()
    }

    fn index_transaction(&mut self, tx_info: &TxInfo) -> anyhow::Result<()> {
        let coinbase = tx_info.tx.is_coinbase();
        for (n, input) in tx_info.tx.input.iter().enumerate() {
            if coinbase {
                break;
            }
            let parent_tx = input.previous_output.txid.into();
            let input_row = schema::Input {
                id: None,
                block: tx_info.block as i64,
                tx_id: tx_info.tx_n,
                tx_hash: tx_info.txid.into(),
                vin: n as i32,
                parent_tx,
                parent_vout: input.previous_output.vout as i32,
            };

            self.state.dataset.new_inputs.push(input_row);
        }

        for (n, out) in tx_info.tx.output.iter().enumerate() {
            let (address_type, address) = match Address::from_script(&out.script_pubkey, self.net) {
                Ok(a) => (
                    a.address_type()
                        .map(|a| a.to_string())
                        .unwrap_or("non_standard".into()),
                    a.to_string(),
                ),
                Err(_) => {
                    let at = if out.script_pubkey.is_op_return() {
                        "op_return"
                    } else if out.script_pubkey.is_multisig() {
                        "multisig"
                    } else {
                        "non_standard"
                    };
                    let address_id = Hash::sha2(out.script_pubkey.as_bytes());
                    (at.into(), format!("nsa_{}", address_id))
                }
            };
            if !self.state.address_index.contains(&address) {
                let address_row = schema::Address {
                    id: None,
                    address: address.clone(),
                    address_type,
                    pk_script: out.script_pubkey.to_bytes(),
                };
                self.state.address_index.insert(address.clone());
                self.state.dataset.new_addresses.push(address_row);
            }
            let utxo = schema::Output {
                id: None,
                block: tx_info.block as i64,
                tx_id: tx_info.tx_n,
                tx_hash: tx_info.txid.into(),
                vout: n as i32,
                amount: out.value.to_sat() as i64,
                coinbase,
                address,
            };
            self.state.dataset.new_outputs.push(utxo);
        }

        Ok(())
    }

    fn commit_state(&mut self) -> anyhow::Result<()> {
        self.state.commit_state()
    }

    fn reset_state(&mut self) {
        self.state.reset_state();
    }
}
