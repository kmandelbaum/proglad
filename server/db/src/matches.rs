use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "matches")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(indexed)]
    pub game_id: i64,
    #[sea_orm(indexed)]
    pub creation_time: TimeDateTimeWithTimeZone,
    #[sea_orm(indexed)]
    pub start_time: Option<TimeDateTimeWithTimeZone>,
    #[sea_orm(indexed)]
    pub end_time: Option<TimeDateTimeWithTimeZone>,
    pub log: Option<Vec<u8>>,
    pub system_message: String,
}


#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::match_participations::Entity")]
    MatchParticipations,
    #[sea_orm(
        belongs_to = "super::games::Entity",
        from = "Column::GameId",
        to = "super::games::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    Games,
    #[sea_orm(has_many = "super::stats_history::Entity")]
    StatsHistory,
}

impl Related<super::games::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Games.def()
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

impl Related<super::bots::Entity> for Entity {
    fn to() -> RelationDef {
        super::match_participations::Relation::Bots.def()
    }

    fn via() -> Option<RelationDef> {
        Some(super::match_participations::Relation::Matches.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
