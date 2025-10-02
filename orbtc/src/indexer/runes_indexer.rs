use std::collections::HashMap;

use anyhow::Context;
use bitcoin::hashes::Hash as _;
use bitcoin::{Address, Transaction, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use orbtc_indexer_api::types::{Amount, Hash};
use ordinals::{Artifact, Edict, RuneId, Runestone, SpacedRune};

use super::db;
use super::rt::{TxIndexer, TxInfo};
use super::runes_indexer_state::State;
use crate::config;
use crate::db::schema;

pub const RUNES_INDEX: &str = "runes_utxo_index";

#[derive(Default, Debug, Clone)]
struct RuneTxsStats {
    etches: u64,
    invalid_etches: u64,
    edicts: u64,
    mints: u64,
    invalid_mints: u64,
    burned_txs: u64,
}

pub struct RunesIndexer {
    net: bitcoin::Network,
    rpc: Client,

    state: State,
    block_stats: RuneTxsStats,
    skip_inputs: bool,

    #[cfg(feature = "test-tweak")]
    pub disable_commitment_validation: bool,
}

impl TxIndexer for RunesIndexer {
    fn name(&self) -> String {
        RUNES_INDEX.into()
    }

    fn index_transaction(&mut self, tx_info: &super::rt::TxInfo) -> anyhow::Result<()> {
        let first_rune_height = ordinals::Rune::first_rune_height(self.net);
        if first_rune_height as u64 > tx_info.block {
            return Ok(());
        }
        if tx_info.tx.is_coinbase() {
            return Ok(());
        }
        self._index_transaction(tx_info)
    }

    fn commit_state(&mut self) -> anyhow::Result<()> {
        info!("Block stats: {:?}", self.block_stats);
        self.block_stats = RuneTxsStats::default();

        self.state.commit_state(self.skip_inputs)
    }

    fn reset_state(&mut self) {
        self.state.reset_state();
    }
}

impl RunesIndexer {
    pub fn new(db_cfg: &config::DBConfig, cfg: &config::BTCConfig, ignore_inputs: bool) -> Self {
        let db = db::DB::establish_connection(&db_cfg.dsn);

        let service_repo = State::new(db);
        let net = cfg.get_network();
        let rpc = Client::new(
            &cfg.address,
            Auth::UserPass(cfg.rpc_user.clone(), cfg.rpc_password.clone()),
        )
        .unwrap();

        Self {
            net,
            rpc,
            state: service_repo,
            skip_inputs: ignore_inputs,

            block_stats: RuneTxsStats::default(),
            #[cfg(feature = "test-tweak")]
            disable_commitment_validation: false,
        }
    }

    fn _index_transaction(&mut self, tx_info: &TxInfo) -> anyhow::Result<()> {
        let artifact = Runestone::decipher(tx_info.tx);

        let mut unallocated = self.unallocated(tx_info.tx)?;
        let mut allocated: Vec<HashMap<RuneId, u128>> =
            vec![HashMap::new(); tx_info.tx.output.len()];

        if let Some(artifact) = &artifact {
            if let Some(id) = artifact.mint() {
                debug!(
                    "RUNE was minted: block={}:{} tx={} {:?}",
                    tx_info.block, tx_info.tx_n, tx_info.txid, id,
                );
                if let Some(amount) = self.mint(id, tx_info.block, &tx_info.txid)? {
                    self.block_stats.mints += 1;
                    *unallocated.entry(id).or_default() += amount;
                }
            }

            let etched = self.etched(tx_info, artifact)?;
            if let Artifact::Runestone(runestone) = artifact {
                if let Some((id, ..)) = etched {
                    *unallocated.entry(id).or_default() +=
                        runestone.etching.unwrap().premine.unwrap_or_default();
                }

                for Edict { id, amount, output } in runestone.edicts.iter().copied() {
                    self.block_stats.edicts += 1;
                    debug!(
                        "RUNE edict: block={} tx={} Edict({id}, {amount}, {output})",
                        tx_info.block, tx_info.tx_n,
                    );

                    // edicts with output values greater than the number of outputs
                    // should never be produced by the edict parser
                    let output = usize::try_from(output).unwrap();
                    assert!(output <= tx_info.tx.output.len());

                    let id = if id == RuneId::default() {
                        let Some((id, ..)) = etched else {
                            continue;
                        };

                        id
                    } else {
                        id
                    };

                    let Some(balance) = unallocated.get_mut(&id) else {
                        continue;
                    };

                    let mut allocate = |balance: &mut u128, amount: u128, output: usize| {
                        if amount > 0 {
                            *balance -= amount;
                            *allocated[output].entry(id).or_default() += amount;
                        }
                    };

                    if output == tx_info.tx.output.len() {
                        // find non-OP_RETURN outputs
                        let destinations = tx_info
                            .tx
                            .output
                            .iter()
                            .enumerate()
                            .filter_map(|(output, tx_out)| {
                                (!tx_out.script_pubkey.is_op_return()).then_some(output)
                            })
                            .collect::<Vec<usize>>();

                        if !destinations.is_empty() {
                            if amount == 0 {
                                // if amount is zero, divide balance between eligible outputs
                                let amount = *balance / destinations.len() as u128;
                                let remainder =
                                    usize::try_from(*balance % destinations.len() as u128).unwrap();

                                for (i, output) in destinations.iter().enumerate() {
                                    allocate(
                                        balance,
                                        if i < remainder { amount + 1 } else { amount },
                                        *output,
                                    );
                                }
                            } else {
                                // if amount is non-zero, distribute amount to eligible outputs
                                for output in destinations {
                                    allocate(balance, amount.min(*balance), output);
                                }
                            }
                        }
                    } else {
                        // Get the allocatable amount
                        let amount = if amount == 0 {
                            *balance
                        } else {
                            amount.min(*balance)
                        };

                        allocate(balance, amount, output);
                    }
                }
            }

            // if let Some((id, rune)) = etched {
            //     self.create_rune_entry(txid, artifact, id, rune)?;
            // }
        }

        let mut burned: HashMap<RuneId, u128> = HashMap::new();

        if let Some(Artifact::Cenotaph(_)) = artifact {
            debug!(
                "CENOTAPH was made: block={}:{} tx={} ",
                tx_info.block, tx_info.tx_n, tx_info.txid,
            );
            for (id, balance) in unallocated {
                *burned.entry(id).or_default() += balance;
            }
        } else {
            let pointer = artifact
                .map(|artifact| match artifact {
                    Artifact::Runestone(runestone) => runestone.pointer,
                    Artifact::Cenotaph(_) => unreachable!(),
                })
                .unwrap_or_default();

            // assign all un-allocated runes to the default output, or the first non
            // OP_RETURN output if there is no default
            if let Some(vout) = pointer
                .map(|pointer| pointer as usize)
                .inspect(|&pointer| assert!(pointer < allocated.len()))
                .or_else(|| {
                    tx_info
                        .tx
                        .output
                        .iter()
                        .enumerate()
                        .find(|(_vout, tx_out)| !tx_out.script_pubkey.is_op_return())
                        .map(|(vout, _tx_out)| vout)
                })
            {
                for (id, balance) in unallocated {
                    if balance > 0 {
                        *allocated[vout].entry(id).or_default() += balance;
                    }
                }
            } else {
                for (id, balance) in unallocated {
                    if balance > 0 {
                        *burned.entry(id).or_default() += balance;
                    }
                }
            }
        }

        // update outpoint balances
        let mut buffer: Vec<u8> = Vec::new();
        for (vout, balances) in allocated.into_iter().enumerate() {
            if balances.is_empty() {
                continue;
            }

            // increment burned balances
            if tx_info.tx.output[vout].script_pubkey.is_op_return() {
                for (id, balance) in &balances {
                    *burned.entry(*id).or_default() += *balance;
                }
                continue;
            }

            buffer.clear();

            let mut balances = balances.into_iter().collect::<Vec<(RuneId, u128)>>();

            // Sort balances by id so tests can assert balances in a fixed order
            balances.sort();

            // let outpoint = OutPoint {
            //     txid: tx_info.tx.txid(),
            //     vout: vout.try_into().unwrap(),
            // };

            // for (id, balance) in balances {
            //     Index::encode_rune_balance(id, balance.n(), &mut buffer);
            // }

            // self.outpoint_to_balances
            //     .insert(&outpoint.store(), buffer.as_slice())?;

            let out = &tx_info.tx.output[vout];

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
                self.state.add_address(address_row);
            }
            for (id, balance) in balances {
                let rune = self.state.get_rune_name_by_id(&id).unwrap();
                let rune_utxo = schema::RuneUtxo {
                    id: None,
                    block: tx_info.block as i64,
                    tx_id: tx_info.tx_n,
                    tx_hash: tx_info.txid.into(),
                    vout: vout as i32,
                    rune,
                    rune_id: format!("{}:{}", id.block, id.tx),
                    address: address.clone(),
                    amount: Amount(balance),
                    btc_amount: out.value.to_sat() as i64,
                };

                self.state.store_new_runes_utxo(rune_utxo);
            }
        }

        if !burned.is_empty() {
            self.block_stats.burned_txs += 1;
        }
        // increment entries with burned runes
        for (id, amount) in burned {
            // *self.burned.entry(id).or_default() += amount;
            self.state.burn_rune_by_id(&id, amount)?;
        }

        Ok(())
    }

    fn unallocated(&mut self, tx: &Transaction) -> anyhow::Result<HashMap<RuneId, u128>> {
        // map of rune ID to un-allocated balance of that rune
        let mut unallocated: HashMap<RuneId, u128> = HashMap::new();

        // increment unallocated runes with the runes in tx inputs
        for input in &tx.input {
            let Some(utxo_list) = self.state.get_parent_utxos(input) else {
                continue;
            };

            for utxo in utxo_list.iter() {
                let rune_id = self
                    .state
                    .get_rune_by_name(&utxo.rune)
                    .context(format!(
                        "get rune({}) for the tx({}) input ",
                        utxo.rune,
                        tx.compute_txid()
                    ))?
                    .rune_id();

                let value = unallocated.entry(rune_id).or_default();
                *value += utxo.amount.0;
            }
        }

        Ok(unallocated)
    }

    fn etched(
        &mut self,
        tx_info: &TxInfo,
        artifact: &Artifact,
    ) -> anyhow::Result<Option<(RuneId, ordinals::Rune)>> {
        let rune = match artifact {
            Artifact::Runestone(runestone) => match runestone.etching {
                Some(etching) => etching.rune,
                None => return Ok(None),
            },
            Artifact::Cenotaph(cenotaph) => match cenotaph.etching {
                Some(rune) => Some(rune),
                None => return Ok(None),
            },
        };

        let (commitment_tx, rune) = if let Some(rune) = rune {
            let height = tx_info.block as u32;
            let minimum = ordinals::Rune::minimum_at_height(self.net, ordinals::Height(height));

            if rune < minimum || rune.is_reserved() {
                warn!(
                    "invalid etching: rune less then min_at_height tx={}",
                    tx_info.txid
                );

                self.block_stats.invalid_etches += 1;
                return Ok(None);
            }

            if self.state.get_rune_by_name(&rune.to_string()).is_ok() {
                warn!(
                    "Rune with such name({}) already exists. Invalid etching block={}:{}",
                    rune, tx_info.block, tx_info.tx_n
                );

                self.block_stats.invalid_etches += 1;
                return Ok(None);
            };

            let Some(commitment_tx) = self.validate_commitment(tx_info, rune) else {
                warn!("invalid etching: invalid commitment tx={}", tx_info.txid);
                self.block_stats.invalid_etches += 1;
                return Ok(None);
            };

            (commitment_tx, rune)
        } else {
            (
                Txid::all_zeros(),
                ordinals::Rune::reserved(tx_info.block, tx_info.tx_n as u32),
            )
        };

        self.block_stats.etches += 1;

        debug!(
            "RUNE({}) was etched: rune_id={}:{} tx={}",
            rune, tx_info.block, tx_info.tx_n, tx_info.txid,
        );

        let rune_row = match artifact {
            Artifact::Cenotaph(_) => schema::Rune {
                block: tx_info.block as i64,
                tx_id: tx_info.tx_n,
                rune_id: format!("{}:{}", tx_info.block, tx_info.tx_n),
                name: rune.to_string(),
                display_name: SpacedRune { rune, spacers: 0 }.to_string(),
                symbol: "¤".into(),
                mints: 0,
                max_supply: Amount(0),
                minted: Amount(0),
                premine: Amount(0),
                burned: Amount(0),
                in_circulation: Amount(0),
                divisibility: 0,
                turbo: false,
                cenotaph: true,
                block_time: tx_info.timestamp,
                etching_tx: tx_info.txid.into(),
                commitment_tx: commitment_tx.into(),
                raw_data: "".into(),
                is_featured: false,
            },

            Artifact::Runestone(runestone) => {
                let etching = runestone.etching;

                let ordinals::Etching {
                    divisibility,
                    premine,
                    spacers,
                    symbol,
                    turbo,
                    ..
                } = etching.unwrap();

                let symbol = {
                    let s = symbol.unwrap_or('¤').to_string();

                    if s.len() <= 1 && !s.as_str().chars().next().unwrap().is_alphabetic() {
                        '¤'.to_string()
                    } else {
                        s
                    }
                };

                let display_name = SpacedRune {
                    rune,
                    spacers: spacers.unwrap_or_default(),
                };

                let max_supply = etching.unwrap().supply().unwrap_or_default();
                let premine = premine.unwrap_or_default();

                let raw_data = serde_json::to_vec(runestone).unwrap_or_default();
                schema::Rune {
                    block: tx_info.block as i64,
                    tx_id: tx_info.tx_n,
                    rune_id: format!("{}:{}", tx_info.block, tx_info.tx_n),
                    name: rune.to_string(),
                    display_name: display_name.to_string(),
                    symbol,
                    mints: 0,
                    max_supply: Amount(max_supply),
                    minted: Amount(premine),
                    premine: Amount(premine),
                    burned: Amount(0),
                    in_circulation: Amount(premine),
                    divisibility: divisibility.unwrap_or_default() as i32,
                    turbo,
                    cenotaph: false,
                    block_time: tx_info.timestamp,
                    etching_tx: tx_info.txid.into(),
                    commitment_tx: commitment_tx.into(),
                    raw_data,
                    is_featured: false,
                }
            }
        };
        if let Err(err) = self.state.store_new_rune(&rune_row) {
            anyhow::bail!("Can't insert rune: error={:#?} rune={:?}", err, rune_row);
        }

        Ok(Some((
            RuneId {
                block: tx_info.block,
                tx: tx_info.tx_n as u32,
            },
            rune,
        )))
    }

    fn mint(&mut self, id: RuneId, height: u64, txid: &Txid) -> anyhow::Result<Option<u128>> {
        let Ok(mut rune_info) = self.state.get_rune_by_id(&id) else {
            warn!(
                "invalid mint: can't get run by id {}:{} tx={txid}",
                id.block, id.tx
            );
            self.block_stats.invalid_mints += 1;
            return Ok(None);
        };

        let Ok(amount) = MintChecker::new(&rune_info).mintable(height) else {
            return Ok(None);
        };

        rune_info.add_mint(amount);
        self.state.update_rune_mint(&rune_info);

        Ok(Some(amount))
    }

    fn validate_commitment(&self, tx_info: &TxInfo, rune: ordinals::Rune) -> Option<Txid> {
        #[cfg(feature = "test-tweak")]
        if self.disable_commitment_validation {
            return Some(Txid::all_zeros());
        }

        let commitment = rune.commitment();

        for input in &tx_info.tx.input {
            // extracting a tapscript does not indicate that the input being spent
            // was actually a taproot output. this is checked below, when we load the
            // output's entry from the database

            #[allow(deprecated)]
            let Some(tapscript) = input.witness.tapscript() else {
                continue;
            };

            for instruction in tapscript.instructions() {
                // ignore errors, since the extracted script may not be valid
                let Ok(instruction) = instruction else {
                    break;
                };

                let Some(pushbytes) = instruction.push_bytes() else {
                    continue;
                };

                if pushbytes.as_bytes() != commitment {
                    continue;
                }
                let commitment_tx = input.previous_output.txid;
                let commitment_tx_info = {
                    let res = self
                        .rpc
                        .get_raw_transaction_info(&input.previous_output.txid, None);
                    match res {
                        Ok(info) => info,
                        Err(err) => {
                            error!(
                                "Can't get parent_tx({}) for etching_tx({}) error={:#?}",
                                input.previous_output.txid, tx_info.txid, err,
                            );
                            return None;
                        }
                    }
                };

                let taproot = commitment_tx_info.vout[input.previous_output.vout as usize]
                    .script_pub_key
                    .script()
                    .unwrap_or_default()
                    .is_p2tr();

                if !taproot {
                    continue;
                }

                let commit_tx_height = match self
                    .rpc
                    .get_block_header_info(&commitment_tx_info.blockhash.unwrap())
                {
                    Ok(bh) => bh.height,
                    Err(err) => {
                        error!(
                            "Can't get block with commitment_tx({}) err={}",
                            commitment_tx, err
                        );
                        return None;
                    }
                };

                let confirmations = tx_info.block - commit_tx_height as u64 + 1;
                if confirmations >= Runestone::COMMIT_CONFIRMATIONS as u64 {
                    return Some(commitment_tx);
                }
            }
        }

        None
    }
}

pub struct MintChecker {
    pub block: u64,
    pub mints: u128,
    pub premine: u128,
    pub terms: Option<ordinals::Terms>,
}

impl MintChecker {
    pub fn new(rune: &crate::db::schema::Rune) -> Self {
        Self {
            block: rune.block as u64,
            mints: rune.mints as u128,
            premine: rune.premine.0,
            terms: rune.terms(),
        }
    }

    pub fn mintable(&self, height: u64) -> Result<u128, MintError> {
        let Some(terms) = self.terms else {
            return Err(MintError::Unmintable);
        };

        if let Some(start) = self.start() {
            if height < start {
                return Err(MintError::Start(start));
            }
        }

        if let Some(end) = self.end() {
            if height >= end {
                return Err(MintError::End(end));
            }
        }

        let cap = terms.cap.unwrap_or_default();

        if self.mints >= cap {
            return Err(MintError::Cap(cap));
        }

        Ok(terms.amount.unwrap_or_default())
    }

    #[allow(dead_code)]
    pub fn supply(&self) -> u128 {
        self.premine
            + self.mints
                * self
                    .terms
                    .and_then(|terms| terms.amount)
                    .unwrap_or_default()
    }

    #[allow(dead_code)]
    pub fn max_supply(&self) -> u128 {
        self.premine
            + self.terms.and_then(|terms| terms.cap).unwrap_or_default()
                * self
                    .terms
                    .and_then(|terms| terms.amount)
                    .unwrap_or_default()
    }

    pub fn start(&self) -> Option<u64> {
        let terms = self.terms?;

        let relative = terms
            .offset
            .0
            .map(|offset| self.block.saturating_add(offset));

        let absolute = terms.height.0;

        relative
            .zip(absolute)
            .map(|(relative, absolute)| relative.max(absolute))
            .or(relative)
            .or(absolute)
    }

    pub fn end(&self) -> Option<u64> {
        let terms = self.terms?;

        let relative = terms
            .offset
            .1
            .map(|offset| self.block.saturating_add(offset));

        let absolute = terms.height.1;

        relative
            .zip(absolute)
            .map(|(relative, absolute)| relative.min(absolute))
            .or(relative)
            .or(absolute)
    }
}

#[derive(Debug, PartialEq)]
pub enum MintError {
    Cap(u128),
    End(u64),
    Start(u64),
    Unmintable,
}

impl std::fmt::Display for MintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MintError::Cap(cap) => write!(f, "limited to {cap} mints"),
            MintError::End(end) => write!(f, "mint ended on block {end}"),
            MintError::Start(start) => write!(f, "mint starts on block {start}"),
            MintError::Unmintable => write!(f, "not mintable"),
        }
    }
}
