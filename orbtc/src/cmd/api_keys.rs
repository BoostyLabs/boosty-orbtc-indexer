use crate::config::Config;
use crate::db;

#[derive(Debug, clap::Parser)]
pub enum ManageApiKeys {
    #[command(about = "Generates and save to the db new API Key")]
    Add(Arg),
    #[command(about = "Blocks API Key with passed name")]
    Block(Arg),
    #[command(about = "List API Keys")]
    List,
}

#[derive(Debug, clap::Parser)]
pub struct Arg {
    #[arg(long)]
    name: String,
}

impl ManageApiKeys {
    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        let cfg = Config::read(cfg_path)?;
        let repo = db::open_postgres_db(&cfg.db).await?;

        match self {
            Self::Add(args) => {
                let row = db::ApiKey::new(&args.name);
                println!("name: {}", args.name);
                println!("key: {}", &row.key);
                repo.insert_api_key(row).await?;
            }
            Self::Block(args) => {
                repo.block_api_key(&args.name).await?;
            }
            Self::List => {
                let keys = repo.select_api_keys().await?;
                println!(" NAME\t KEY\t BLOCKED\t CAN_LOCK_UTXO");
                for key in keys {
                    println!(
                        "{}\t {}\t {}\t {}",
                        key.name, key.key, key.blocked, key.can_lock_utxo,
                    )
                }
            }
        }

        Ok(())
    }
}
