use proglad_db::{files, prelude::*};
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
        let mut create_table = s.create_table_from_entity(Files);
        create_table.if_not_exists();
        m.create_table(create_table).await?;
        for mut i in idx(&s, Files) {
            i.if_not_exists();
            m.create_index(i).await?;
        }
        let mut owning_entity_and_name_index = Index::create();
        owning_entity_and_name_index
            .name("files-owning-entity-and-name-index")
            .if_not_exists()
            .table(Files);
        owning_entity_and_name_index.col(files::Column::OwningEntity);
        owning_entity_and_name_index.col(files::Column::OwningId);
        owning_entity_and_name_index.col(files::Column::Name);
        owning_entity_and_name_index.unique();
        m.create_index(owning_entity_and_name_index).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(Files).if_exists().to_owned())
            .await?;
        Ok(())
    }
}
