use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum WorkType {
    #[sea_orm(string_value = "compilation")]
    Compilation,
    #[sea_orm(string_value = "runmatch")]
    RunMatch,
}

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum Status {
    #[sea_orm(string_value = "scheduled")]
    Scheduled,
    #[sea_orm(string_value = "started")]
    Started,
    #[sea_orm(string_value = "completed")]
    Completed,
    #[sea_orm(string_value = "canceled")]
    Canceled,
    #[sea_orm(string_value = "failed")]
    Failed,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "work_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(indexed)]
    pub creation_time: TimeDateTimeWithTimeZone,
    pub start_time: Option<TimeDateTimeWithTimeZone>,
    pub end_time: Option<TimeDateTimeWithTimeZone>,
    pub work_type: WorkType,
    #[sea_orm(indexed)]
    pub status: Status,
    pub game_id: Option<i64>,
    pub program_id: Option<i64>,
    pub match_id: Option<i64>,
    pub priority: i64, // Greater value is higher.
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::games::Entity",
        from = "Column::GameId",
        to = "super::games::Column::Id",
        on_update = "Cascade",
        on_delete = "SetNull"
    )]
    Games,
    #[sea_orm(
        belongs_to = "super::matches::Entity",
        from = "Column::MatchId",
        to = "super::matches::Column::Id",
        on_update = "Cascade",
        on_delete = "SetNull"
    )]
    Matches,
    #[sea_orm(
        belongs_to = "super::programs::Entity",
        from = "Column::ProgramId",
        to = "super::programs::Column::Id",
        on_update = "Cascade",
        on_delete = "SetNull"
    )]
    Programs,
}

impl Related<super::games::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Games.def()
    }
}

impl Related<super::matches::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Matches.def()
    }
}

impl Related<super::programs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Programs.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
