mod bitcoin_indexer;
mod bitcoin_indexer_state;
mod inscriptions_index;

pub mod db;
mod rt;
mod runes_indexer;
mod runes_indexer_state;

use std::time;

pub use bitcoin_indexer::BITCOIN_INDEX;
pub use inscriptions_index::{
    InscriptionsCacheIndexer, InscriptionsCacher, INSCRIPTIONS_CACHE_INDEX,
};
pub use rt::{BlockIndexerRt, IndexerType, IndexingOpts, TxIndexer, TxInfo};
pub use runes_indexer::{RunesIndexer, RUNES_INDEX};

static mut INDEXER_WAIT_INTERVAL: time::Duration = time::Duration::from_secs(5);

/// This method is intended for use only within integration tests.
pub fn set_indexer_wait_interval(nt: time::Duration) {
    unsafe {
        INDEXER_WAIT_INTERVAL = nt;
    }
}
