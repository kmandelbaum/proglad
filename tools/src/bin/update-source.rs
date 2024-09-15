use anyhow::anyhow;
use clap::Parser;
use sea_orm::prelude::TimeDateTimeWithTimeZone;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter
};
use sea_orm::Set;

use proglad_db as db;

#[derive(Parser, Debug)]
struct Config {
    #[arg(long)]
    db: String,
    #[arg(long, short='p')]
    program: Option<i64>,
    #[arg(long, short='b')]
    bot: Option<String>,
    #[arg(long, short='g')]
    game: Option<String>,
    #[arg(long, short='f')]
    filename: String,
    #[arg(long)]
    skip_status_reset: bool,
}

async fn program_id(cfg: &Config, db: &DatabaseConnection) -> anyhow::Result<i64> {
    match (cfg.program, &cfg.bot, &cfg.game) {
        (Some(program), None, None) => Ok(program),
        (None, Some(_), None) => todo!(),
        (None, None, Some(game_name)) => {
            if let Some(game) = db::games::Entity::find()
                .filter(db::games::Column::Name.eq(game_name))
                .one(db)
                .await? {
                Ok(game.program_id)
            } else {
                Err(anyhow!("Game {game_name} not found"))
            }
        }
        _ => Err(anyhow!("Exactly one filter (p|g|b) must be specified.")),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let db = sea_orm::Database::connect(&cfg.db).await?;
    let id = program_id(&cfg, &db).await?;
    let contents = tokio::fs::read_to_string(&cfg.filename).await?;
    let mut update = db::programs::ActiveModel {
        id: Set(id),
        source_code: Set(Some(contents)),
        ..Default::default()
    };
    println!("Program id: {id}");
    if !cfg.skip_status_reset {
        println!("Will reset the program status to force recompilation.");
        update.status = Set(db::programs::Status::New);
        update.status_reason = Set(Some("Reset by update-source".to_owned()));
        update.status_update_time = Set(TimeDateTimeWithTimeZone::now_utc());
    } else {
        println!("Skipping status reset. The program won't be recompiled");
    }
    db::programs::Entity::update(update).exec(&db).await?;
    println!("Updated successfully.");
    Ok(())
}
