use proglad_db::{matches, prelude::*};
use sea_orm::EntityTrait;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

fn idx<E: EntityTrait>(s: &sea_orm::Schema, e: E) -> Vec<IndexCreateStatement> {
    s.create_index_from_entity(e)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let s = sea_orm::Schema::new(sea_orm::DatabaseBackend::Sqlite);
        for mut i in idx(&s, Matches).into_iter() {
            i.if_not_exists();
            m.create_index(i).await?;
        }
        let mut game_id_end_time_index = Index::create();
        game_id_end_time_index
            .name("game-id-end-time-index")
            .if_not_exists()
            .table(Matches)
            .col(matches::Column::GameId)
            .col(matches::Column::EndTime);
        m.create_index(game_id_end_time_index).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
