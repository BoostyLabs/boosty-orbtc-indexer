use clap::Parser;

use crate::config::Config;
use crate::db;

#[derive(Debug, Parser)]
pub enum DbCmd {
    #[command(about = "Apply migrations")]
    MigrateUp,
    #[command(
        about = "Drop indexes on some tables. It's required to speedup initial indexing process"
    )]
    DropIndexes,
    #[command(about = "Restore indexes when initial indexing is done")]
    RestoreIndexes,
    #[command(about = "Prints migrations metadata")]
    ListMigrations,
}

impl DbCmd {
    pub async fn run(&self, cfg_path: &str) -> anyhow::Result<()> {
        match self {
            DbCmd::MigrateUp => migrate_up(cfg_path).await,
            DbCmd::DropIndexes => drop_indexes(cfg_path).await,
            DbCmd::RestoreIndexes => restore_indexes(cfg_path).await,
            DbCmd::ListMigrations => {
                println!("MIGRATIONS:");
                for m in db::get_migration_info() {
                    println!("-> {}\t{}\t{}", m.0, m.1, hex::encode(m.2))
                }
                Ok(())
            }
        }
    }
}

pub async fn migrate_up(cfg_path: &str) -> anyhow::Result<()> {
    let cfg = Config::read(cfg_path)?;
    db::apply_migrations(&cfg.db).await?;
    Ok(())
}

pub async fn drop_indexes(cfg_path: &str) -> anyhow::Result<()> {
    let cfg = Config::read(cfg_path)?;

    db::apply_migrations(&cfg.db).await?;
    let repo = db::open_postgres_db(&cfg.db).await?;
    for (name, _) in indexes() {
        log::info!("drop index({name})");
        sqlx::query(&format!("DROP INDEX IF EXISTS {name}"))
            .execute(&repo.pool)
            .await?;
        log::info!("done ({name})");
    }

    Ok(())
}

pub async fn restore_indexes(cfg_path: &str) -> anyhow::Result<()> {
    let cfg = Config::read(cfg_path)?;

    db::apply_migrations(&cfg.db).await?;
    let repo = db::open_postgres_db(&cfg.db).await?;
    for (name, terms) in indexes() {
        log::info!("restore index({name})");
        sqlx::query(&format!("CREATE INDEX IF NOT EXISTS {name} ON {terms}"))
            .execute(&repo.pool)
            .await?;
        log::info!("done ({name})");
    }

    Ok(())
}

fn indexes() -> [(&'static str, &'static str); 7] {
    [
        ("idx_outputs_address", "outputs(address)"),
        ("idx_outputs_amount", "outputs(amount)"),
        ("idx_outputs_block", "outputs(block)"),
        ("idx_outputs_tx_vout", "outputs(tx_hash, vout)"),
        ("idx_inputs_block", "inputs(block)"),
        (
            "idx_inputs_parent_tx_vout",
            "inputs(parent_tx, parent_vout)",
        ),
        ("idx_inputs_tx_vout", "inputs(tx_hash, vin)"),
    ]
}
