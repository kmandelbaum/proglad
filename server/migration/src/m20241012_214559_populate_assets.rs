use proglad_db::{common, files, games, prelude::*};
use sea_orm::entity::prelude::TimeDateTimeWithTimeZone;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect, Set};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if !std::env::var("PROGLAD_POPULATE_DATABASE").is_ok() {
            return Ok(());
        }
        let db = manager.get_connection();
        let halma_quad_id = get_game_id(db, "halma-quad").await?;
        write_asset_file(
            db,
            halma_quad_id,
            "../games/halma-quad/icon.svg",
            files::ContentType::Svg,
            files::Kind::StaticContent,
        )
        .await?;
        write_asset_file(
            db,
            halma_quad_id,
            "../games/halma-quad/index.html",
            files::ContentType::Html,
            files::Kind::StaticContent,
        )
        .await?;
        write_asset_file(
            db,
            halma_quad_id,
            "../games/halma-quad/initial.svg",
            files::ContentType::Svg,
            files::Kind::StaticContent,
        )
        .await?;
        let lowest_unique_id = get_game_id(db, "lowest-unique").await?;
        write_asset_file(
            db,
            lowest_unique_id,
            "../games/lowest-unique/icon.svg",
            files::ContentType::Svg,
            files::Kind::StaticContent,
        )
        .await?;
        write_asset_file(
            db,
            lowest_unique_id,
            "../games/lowest-unique/index.html",
            files::ContentType::Html,
            files::Kind::StaticContent,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let mut ids = vec![];
        if let Ok(halma_quad_id) = get_game_id(db, "halma-quad").await {
            ids.push(halma_quad_id);
        }
        if let Ok(lowest_unique_id) = get_game_id(db, "lowest-unique").await {
            ids.push(lowest_unique_id);
        }
        if ids.is_empty() {
            return Ok(());
        }
        Files::delete_many()
            .filter(
                Condition::all()
                    .add(files::Column::OwningEntity.eq(common::EntityKind::Game))
                    .add(files::Column::OwningId.is_in(ids))
                    .add(files::Column::Kind.eq(files::Kind::StaticContent)),
            )
            .exec(db)
            .await?;
        Ok(())
    }
}

async fn get_game_id<C: ConnectionTrait>(db: &C, game_name: &str) -> Result<i64, DbErr> {
    Games::find()
        .filter(games::Column::Name.eq(game_name))
        .select_only()
        .column(games::Column::Id)
        .into_values::<i64, games::Column>()
        .one(db)
        .await?
        .ok_or_else(|| DbErr::Custom(format!("Failed to find game id for {game_name}")))
}

async fn write_asset_file<C: ConnectionTrait>(
    db: &C,
    game_id: i64,
    path: impl AsRef<std::path::Path> + std::fmt::Debug,
    content_type: files::ContentType,
    kind: files::Kind,
) -> Result<(), DbErr> {
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| DbErr::Custom(format!("failed to read asset file {path:?}: {e:?}")))?;
    let name = path
        .as_ref()
        .file_name()
        .map_or(String::new(), |s| s.to_string_lossy().to_string());
    let file = files::ActiveModel {
        owning_entity: Set(common::EntityKind::Game),
        owning_id: Set(Some(game_id)),
        content_type: Set(content_type),
        kind: Set(kind),
        content: Set(Some(content.into_bytes())),
        last_update: Set(TimeDateTimeWithTimeZone::now_utc()),
        name: Set(name),
        compression: Set(files::Compression::Uncompressed),
        ..Default::default()
    };
    Files::insert(file).exec(db).await?;
    Ok(())
}
