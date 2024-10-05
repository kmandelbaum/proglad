use clap::Parser;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, TransactionTrait};

use proglad_db as db;
use proglad_server::engine;

#[derive(Parser, Debug)]
struct Config {
    #[arg(long)]
    db: String,
    #[arg(long)]
    game_id: i64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let db = sea_orm::Database::connect(cfg.db).await?;
    let matches = db::matches::Entity::find()
        .filter(db::matches::Column::GameId.eq(cfg.game_id))
        .order_by_asc(db::matches::Column::EndTime)
        .all(&db)
        .await?;
    for m in matches {
        println!("Processing match {}", m.id);
        let participations = db::match_participations::Entity::find()
            .filter(db::match_participations::Column::MatchId.eq(m.id))
            .all(&db)
            .await?;
        let scores = participations
            .into_iter()
            .filter_map(|p| p.score.map(|s| (p.bot_id, s)))
            .collect::<Vec<_>>();
        if scores.is_empty() {
            continue;
        }
        db.transaction(|txn| {
            Box::pin(async move {
                engine::db_update_stats_for_match(txn, m.id, scores).await?;
                Ok::<(), engine::MyDbError>(())
            })
        })
        .await?;
    }
    Ok(())
}
