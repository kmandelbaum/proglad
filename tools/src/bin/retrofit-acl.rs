use clap::Parser;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};

use proglad_db as db;

#[derive(Parser, Debug)]
struct Config {
    #[arg(long)]
    db: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let db = sea_orm::Database::connect(&cfg.db).await?;
    let public_program_ids = db::programs::Entity::find()
        .filter(db::programs::Column::IsPublic.eq(Some(true)))
        .select_only()
        .column(db::programs::Column::Id)
        .into_values::<i64, db::programs::Column>()
        .all(&db)
        .await?;
    for id in public_program_ids {
        db::acls::set_program_public(&db, id, true).await?;
    }
    let game_ids = db::games::Entity::find()
        .select_only()
        .column(db::games::Column::Id)
        .into_values::<i64, db::games::Column>()
        .all(&db)
        .await?;
    for id in game_ids {
        db::acls::set_game_public(&db, id, true).await?;
    }
    Ok(())
}
