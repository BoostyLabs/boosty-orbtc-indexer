use std::time::Duration;

use anyhow::Context;
use bb8::Pool;
use bb8_redis::redis::AsyncCommands;
use bb8_redis::RedisConnectionManager;

const FBTC_LOCKS_PREFIX: &str = "orbtc:utxo_locks";
const NO_ID: &str = "p.j.fry";

#[derive(Clone)]
pub struct Repo {
    pub pool: Pool<RedisConnectionManager>,
    lock_ttl: u64,
}

impl Repo {
    pub async fn new(url: &str, lock_ttl: u64) -> anyhow::Result<Self> {
        let manager = RedisConnectionManager::new(url)
            .with_context(|| format!("invalid redis url: {}", url))?;

        let pool = Pool::builder()
            .max_size(100)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)
            .await
            .with_context(|| format!("can't create redis pool for {}", url))?;
        let lock_ttl = if lock_ttl > 0 { lock_ttl } else { 25 };
        Ok(Self { pool, lock_ttl })
    }

    pub async fn lock_utxo(
        &self,
        tx_hash: &orbtc_indexer_api::types::Hash,
        vout: i32,
        request_id: &str,
    ) -> anyhow::Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("{}:{}:{}", FBTC_LOCKS_PREFIX, tx_hash, vout);
        let id = if request_id.is_empty() {
            format!("{NO_ID}-{}", rand::random::<u32>())
        } else {
            request_id.to_owned()
        };

        conn.set_ex::<String, String, ()>(key, id, self.lock_ttl)
            .await?;
        Ok(())
    }

    pub async fn check_is_locked(
        &self,
        tx_hash: &orbtc_indexer_api::types::Hash,
        vout: i32,
        request_id: &Option<String>,
    ) -> anyhow::Result<bool> {
        let mut conn = self.pool.get().await?;
        let key = format!("{}:{}:{}", FBTC_LOCKS_PREFIX, tx_hash, vout);
        let data: Option<String> = conn.get(key).await?;

        match data {
            Some(val) => {
                if val == NO_ID {
                    return Ok(true);
                }
                // if request_id matches,
                // this is the same session or repeated request,
                // so we treat it like it is not locked
                Ok(!Some(val).eq(request_id))
            }
            None => Ok(false),
        }
    }
}
