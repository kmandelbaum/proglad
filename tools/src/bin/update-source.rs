use anyhow::anyhow;
use clap::Parser;
use sea_orm::prelude::TimeDateTimeWithTimeZone;
use sea_orm::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use proglad_db as db;
use proglad_server::file_store;

#[derive(Parser, Debug)]
struct Config {
    #[arg(long)]
    db: String,
    #[arg(long, short = 'p')]
    program: Option<i64>,
    #[arg(long, short = 'b')]
    bot: Option<String>,
    #[arg(long, short = 'g')]
    game: Option<String>,
    #[arg(long, short = 'f')]
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
                .await?
            {
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
    let content = tokio::fs::read_to_string(&cfg.filename).await?;
    let fs = file_store::FileStore::new();
    let file = db::files::Model {
        owning_entity: db::common::EntityKind::Program,
        owning_id: Some(id),
        content_type: db::files::ContentType::PlainText,
        kind: db::files::Kind::SourceCode,
        content: Some(content.into_bytes()),
        last_update: TimeDateTimeWithTimeZone::now_utc(),
        name: String::new(),
        compression: db::files::Compression::Uncompressed,
        ..Default::default()
    };
    let file = file_store::FileStore::compress(file)?;
    fs.write(&db, file_store::Requester::System, file).await?;
    println!("Program id: {id}");
    if !cfg.skip_status_reset {
        println!("Will reset the program status to force recompilation.");
        let update = db::programs::ActiveModel {
            id: Set(id),
            status: Set(db::programs::Status::New),
            status_reason: Set(Some("Reset by update-source".to_owned())),
            status_update_time: Set(TimeDateTimeWithTimeZone::now_utc()),
            ..Default::default()
        };
        db::programs::Entity::update(update).exec(&db).await?;
    } else {
        println!("Skipping status reset. The program won't be recompiled");
    }
    println!("Updated successfully.");
    Ok(())
}
