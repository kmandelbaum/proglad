use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "accounts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique, indexed)]
    pub name: String,
    #[sea_orm(unique, indexed)]
    pub email: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::bots::Entity")]
    Bots,
}

impl Related<super::bots::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bots.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
