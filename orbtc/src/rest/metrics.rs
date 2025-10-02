use std::sync::LazyLock;

use orbtc_indexer_api::StatusResponse;
use prometheus::core::{AtomicU64, GenericGauge};
use prometheus::Registry;

static STATE: LazyLock<State> = LazyLock::new(|| match State::new() {
    Ok(state) => state,
    Err(err) => {
        error!("metrics state can't be initialized: error={err:#}");
        panic!("metrics state can't be initialized");
    }
});

pub fn registry() -> Registry {
    STATE.registry.clone()
}

pub fn update(status: StatusResponse) {
    STATE.update(status);
}

struct State {
    registry: Registry,
    last_block_btc: GenericGauge<AtomicU64>,
    last_indexed_block_btc: GenericGauge<AtomicU64>,
    last_indexed_block_runes: GenericGauge<AtomicU64>,
}

impl State {
    fn new() -> anyhow::Result<Self> {
        let shared_registry = Registry::new();
        let last_block = GenericGauge::new("last_block_network", "Best block in bitcoin network")?;
        let last_indexed_block_btc = GenericGauge::new(
            "last_block_btc_indexer",
            "Last indexed block by btc indexer",
        )?;
        let last_indexed_block_runes = GenericGauge::new(
            "last_block_runes_indexer",
            "Last indexed block by runes indexer",
        )?;

        shared_registry.register(Box::new(last_block.clone()))?;
        shared_registry.register(Box::new(last_indexed_block_btc.clone()))?;
        shared_registry.register(Box::new(last_indexed_block_runes.clone()))?;
        Ok(Self {
            registry: shared_registry,
            last_block_btc: last_block,
            last_indexed_block_btc,
            last_indexed_block_runes,
        })
    }

    fn update(&self, status: StatusResponse) {
        self.last_block_btc.set(status.btc_height);
        self.last_indexed_block_btc.set(status.btc_indexer_height);
        self.last_indexed_block_runes
            .set(status.runes_indexer_height);
    }
}
