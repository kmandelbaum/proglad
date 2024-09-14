use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, DeriveEntityModel)]
#[sea_orm(table_name = "stats_history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(indexed)]
    pub bot_id: i64,
    #[sea_orm(indexed)]
    pub latest: bool,
    #[sea_orm(indexed)]
    pub update_time: TimeDateTimeWithTimeZone,
    pub match_id: Option<i64>,
    pub total_score: f64,
    pub total_matches: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::bots::Entity",
        from = "Column::BotId",
        to = "super::bots::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Bots,
    #[sea_orm(
        belongs_to = "super::matches::Entity",
        from = "Column::MatchId",
        to = "super::matches::Column::Id",
        on_update = "Cascade",
        on_delete = "SetNull"
    )]
    Matches,
}

impl Related<super::bots::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bots.def()
    }
}

impl Related<super::matches::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Matches.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
