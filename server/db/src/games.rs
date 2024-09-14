use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum Status {
    #[sea_orm(string_value = "inactive")]
    Inactive,
    #[sea_orm(string_value = "active")]
    Active,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "games")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique, indexed)]
    pub name: String,
    pub description: String,
    pub min_players: i32,
    pub max_players: i32,
    pub program_id: i64,
    pub status: Status,
    // Supports %%-substitutions
    pub param: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::bots::Entity")]
    Bots,
    #[sea_orm(
        belongs_to = "super::programs::Entity",
        from = "Column::ProgramId",
        to = "super::programs::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    Programs,
    #[sea_orm(has_many = "super::matches::Entity")]
    Matches,
}

impl Related<super::bots::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bots.def()
    }
}

impl Related<super::programs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Programs.def()
    }
}

impl Related<super::matches::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Matches.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
