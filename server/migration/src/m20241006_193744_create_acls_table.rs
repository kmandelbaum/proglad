use proglad_db::{acls, prelude::*};
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
        let mut create_table = s.create_table_from_entity(Acls);
        create_table.if_not_exists();
        m.create_table(create_table).await?;
        for i in idx(&s, Acls) {
            m.create_index(i).await?;
        }
        let mut acl_entity_index = Index::create();
        acl_entity_index
            .name("acl-entity-index")
            .if_not_exists()
            .table(Acls);
        acl_entity_index.col(acls::Column::EntityKind);
        acl_entity_index.col(acls::Column::EntityId);
        m.create_index(acl_entity_index).await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(Acls).if_exists().to_owned())
            .await?;
        Ok(())
    }
}
