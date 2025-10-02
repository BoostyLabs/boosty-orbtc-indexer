use std::str::FromStr;

use bigdecimal::{BigDecimal, FromPrimitive};
use bitcoin::hashes::Hash;
use bitcoin::Txid;
use orbtc_indexer_api::Rune;
use ordinals::{Etching, Runestone, SpacedRune, Terms};

pub const FIRST_RUNE: &str = "UNCOMMONGOODS";

pub fn reserved_rune() -> Rune {
    let sp = SpacedRune::from_str("UNCOMMON•GOODS").unwrap();

    let etching = Etching {
        divisibility: Some(0),
        symbol: Some('⧉'),
        turbo: true,
        rune: Some(sp.rune),
        spacers: Some(sp.spacers),
        premine: Some(0),
        terms: Some(Terms {
            amount: Some(1),
            cap: Some(340282366920938463463374607431768211455),
            height: (Some(840000), Some(1050000)),
            offset: (None, None),
        }),
    };
    let runestone = Runestone {
        etching: Some(etching),
        mint: None,
        pointer: None,
        edicts: Vec::new(),
    };

    let max_supply: u128 = etching.supply().unwrap_or_default();
    let max_supply = BigDecimal::from_u128(max_supply).unwrap_or_default();
    let premine = BigDecimal::from(etching.premine.unwrap_or_default());

    let raw_data = serde_json::to_vec(&runestone).unwrap_or_default();
    Rune {
        block: 1,
        tx_id: 0,
        rune_id: "1:0".into(),
        name: sp.rune.to_string(),
        display_name: sp.to_string(),
        symbol: etching.symbol.unwrap_or('¤').to_string(),
        mints: 0,
        premine: 0.into(),
        burned: 0.into(),
        max_supply,
        minted: premine.clone(),
        in_circulation: premine,
        divisibility: etching.divisibility.unwrap_or_default() as i32,
        turbo: etching.turbo,
        block_time: 0,
        etching_tx: Txid::all_zeros().into(),
        commitment_tx: Txid::all_zeros().into(),
        raw_data,
        is_featured: false,
    }
}
