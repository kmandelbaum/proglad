use proglad_db::{prelude::*, work_items};
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
        let mut create_table = s.create_table_from_entity(WorkItems);
        create_table.if_not_exists();
        m.create_table(create_table).await?;
        for mut i in idx(&s, WorkItems) {
            i.if_not_exists();
            m.create_index(i).await?;
        }
        let mut status_priority_index = Index::create();
        status_priority_index
            .name("work-item-status-priority-creation-time-index")
            .if_not_exists()
            .table(WorkItems)
            .col(work_items::Column::Status)
            .col(work_items::Column::Priority)
            .col(work_items::Column::CreationTime);
        m.create_index(status_priority_index).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
