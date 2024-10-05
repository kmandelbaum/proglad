use anyhow::anyhow;
use clap::Parser;
use sea_orm::prelude::TimeDateTimeWithTimeZone;
use sea_orm::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use proglad_db as db;
use proglad_server::file_store::{self, FileStore};

#[derive(Parser, Debug)]
struct Config {
    #[arg(long)]
    db: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let db = sea_orm::Database::connect(&cfg.db).await?;
    let programs = db::programs::Entity::find().all(&db).await?;
    let file_store = FileStore {};
    for p in programs {
        let Some(source_code) = p.source_code else {
            continue;
        };
        if source_code.is_empty() {
            continue;
        }
        let file = FileStore::compress(db::files::Model {
            owning_entity: db::files::OwningEntity::Program,
            owning_id: Some(p.id),
            kind: db::files::Kind::SourceCode,
            content_type: db::files::ContentType::PlainText,
            content: Some(source_code.into_bytes()),
            ..Default::default()
        })?;
        file_store
            .write(&db, file_store::Requester::System, file)
            .await?;
        println!("Moved source for program {}", p.id);
    }
    Ok(())
}
