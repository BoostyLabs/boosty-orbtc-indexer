use std::str::FromStr;

use bitcoin::hashes::Hash;
use hex;
use sf::firehose::v2::SingleBlockResponse;
use tonic::transport::{Channel, ClientTlsConfig};
use tonic::Request;

use prost::Message;

mod sf;

// Generated from Firehose proto
use sf::bitcoin::v1::Block as BtcBlock;
use sf::firehose::v2::{
    fetch_client::FetchClient as FirehoseClient,
    single_block_request::{BlockNumber, Reference},
    SingleBlockRequest,
};

const FIREHOSE_BTC: &str = "https://mainnet.btc.streamingfast.io:443";

pub struct FHClient {
    api_key: String,
    client: FirehoseClient<Channel>,
}

impl FHClient {
    pub fn new(api_key: &str) -> anyhow::Result<Self> {
        use tokio::runtime::Runtime;
        let rt = Runtime::new()?;
        rt.block_on(async {
            let channel = Channel::from_static(FIREHOSE_BTC)
                .tls_config(ClientTlsConfig::new().with_webpki_roots())?
                .connect()
                .await?;

            let client = FirehoseClient::new(channel)
                .max_decoding_message_size(30417402)
                .accept_compressed(tonic::codec::CompressionEncoding::Gzip);
            Ok(Self {
                api_key: api_key.to_owned(),
                client,
            })
        })
    }

    pub fn get_block(
        &mut self,
        block_num: u64,
    ) -> anyhow::Result<(bitcoin::BlockHash, bitcoin::Block)> {
        use tokio::runtime::Runtime;
        let rt = Runtime::new()?;

        let block: anyhow::Result<SingleBlockResponse> = rt.block_on(async {
            let mut request = Request::new(SingleBlockRequest {
                reference: Some(Reference::BlockNumber(BlockNumber { num: block_num })),
                ..Default::default()
            });
            request
                .metadata_mut()
                .insert("authorization", format!("Bearer {}", self.api_key).parse()?);

            let response = self.client.block(request).await?;
            let block = response.into_inner();
            Ok(block)
        });

        let block = block?;
        let Some(data) = block.block else {
            anyhow::bail!("empty block")
        };

        let proto_block = BtcBlock::decode(data.value.as_slice())?;
        let block = proto_block_to_btc(proto_block)?;
        Ok(block)
    }
}

fn proto_block_to_btc(problock: BtcBlock) -> anyhow::Result<(bitcoin::BlockHash, bitcoin::Block)> {
    use bitcoin::block::{Header, Version};
    use bitcoin::{BlockHash, CompactTarget, Transaction, TxMerkleNode};

    let block_hash = BlockHash::from_str(&problock.hash)?;
    let mut block = bitcoin::Block {
        header: Header {
            version: Version::from_consensus(problock.version),
            prev_blockhash: BlockHash::from_str(&problock.previous_hash)?,
            merkle_root: TxMerkleNode::from_str(&problock.merkle_root)?,
            time: problock.time as u32,
            bits: CompactTarget::from_unprefixed_hex(&problock.bits)?,
            nonce: problock.nonce,
        },
        txdata: Vec::with_capacity(problock.tx.len()),
    };

    for ptx in problock.tx.iter() {
        use bitcoin::locktime::absolute::LockTime;
        use bitcoin::transaction::Version;
        let mut tx = Transaction {
            version: Version(ptx.version as i32),
            lock_time: LockTime::from_consensus(ptx.locktime),
            input: Vec::with_capacity(ptx.vin.len()),
            output: Vec::with_capacity(ptx.vout.len()),
        };

        for vin in ptx.vin.iter() {
            use bitcoin::{OutPoint, ScriptBuf, Sequence, TxIn, Txid, Witness};

            let script_sig = if let Some(sig) = vin.script_sig.as_ref() {
                ScriptBuf::from_hex(&sig.hex)?
            } else {
                ScriptBuf::default()
            };
            let witnesses: Vec<_> = vin
                .txinwitness
                .iter()
                .filter_map(|e| hex::decode(e).ok())
                .collect();

            let witness = if witnesses.is_empty() {
                Witness::default()
            } else {
                Witness::from_slice(&witnesses)
            };
            let txid = if !vin.coinbase.is_empty() {
                Txid::all_zeros()
            } else {
                Txid::from_str(&vin.txid)?
            };
            let input = TxIn {
                previous_output: OutPoint {
                    txid,
                    vout: vin.vout,
                },
                script_sig,
                sequence: Sequence(vin.sequence),
                witness,
            };
            tx.input.push(input);
        }

        for vout in ptx.vout.iter() {
            use bitcoin::{Amount, ScriptBuf, TxOut};
            let script_pubkey = if let Some(sig) = vout.script_pub_key.as_ref() {
                ScriptBuf::from_hex(&sig.hex)?
            } else {
                ScriptBuf::default()
            };

            let out = TxOut {
                value: Amount::from_btc(vout.value)?,
                script_pubkey,
            };

            tx.output.push(out);
        }

        block.txdata.push(tx);
    }

    Ok((block_hash, block))
}
