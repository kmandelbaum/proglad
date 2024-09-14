use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum OwnerSetStatus {
    #[sea_orm(string_value = "inactive")]
    Inactive,
    #[sea_orm(string_value = "active")]
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum SystemStatus {
    #[sea_orm(string_value = "unknown")]
    Unknown,
    #[sea_orm(string_value = "ok")]
    Ok,
    #[sea_orm(string_value = "deactivated")]
    Deactivated,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "bots")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    #[sea_orm(indexed)]
    pub owner_id: i64,
    #[sea_orm(indexed)]
    pub game_id: i64,
    #[sea_orm(indexed)]
    pub program_id: i64,
    pub owner_set_status: OwnerSetStatus,
    pub system_status: SystemStatus,
    pub system_status_reason: Option<String>,
    pub creation_time: TimeDateTimeWithTimeZone,
    pub status_update_time: TimeDateTimeWithTimeZone,
    #[sea_orm(indexed)]
    pub is_reference_bot: Option<bool>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::accounts::Entity",
        from = "Column::OwnerId",
        to = "super::accounts::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Accounts,
    #[sea_orm(
        belongs_to = "super::games::Entity",
        from = "Column::GameId",
        to = "super::games::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    Games,
    #[sea_orm(
        belongs_to = "super::programs::Entity",
        from = "Column::ProgramId",
        to = "super::programs::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    Programs,
    #[sea_orm(has_many = "super::match_participations::Entity")]
    MatchParticipations,
    #[sea_orm(has_many = "super::stats_history::Entity")]
    StatsHistory,
}

impl Related<super::accounts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Accounts.def()
    }
}

impl Related<super::games::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Games.def()
    }
}

impl Related<super::programs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Programs.def()
    }
}

impl Related<super::match_participations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MatchParticipations.def()
    }
}

impl Related<super::stats_history::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::StatsHistory.def()
    }
}

impl Related<super::matches::Entity> for Entity {
    fn to() -> RelationDef {
        super::match_participations::Relation::Matches.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::match_participations::Relation::Bots.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
