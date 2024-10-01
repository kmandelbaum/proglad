use proglad_db::{prelude::*, stats_history};
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
        let s = sea_orm::Schema::new(m.get_database_backend());
        let mut create_table = s.create_table_from_entity(StatsHistory);
        create_table.if_not_exists();
        m.create_table(create_table).await?;
        for mut i in idx(&s, StatsHistory) {
            i.if_not_exists();
            m.create_index(i).await?;
        }
        let mut bot_id_update_time_index = Index::create();
        bot_id_update_time_index
            .name("bot-id-update-time-index")
            .if_not_exists()
            .table(StatsHistory)
            .col(stats_history::Column::BotId)
            .col(stats_history::Column::UpdateTime);
        m.create_index(bot_id_update_time_index).await?;
        let mut latest_bot_id_index = Index::create();
        latest_bot_id_index
            .name("latest-bot-id-index")
            .if_not_exists()
            .table(StatsHistory)
            .col(stats_history::Column::Latest)
            .col(stats_history::Column::BotId);
        m.create_index(latest_bot_id_index).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
