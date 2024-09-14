use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "match_participations")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub match_id: i64,
    #[sea_orm(indexed)]
    pub bot_id: i64,
    #[sea_orm(primary_key)]
    pub ingame_player: u32,
    pub score: Option<f64>,
    pub system_message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::matches::Entity",
        from = "Column::MatchId",
        to = "super::matches::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Matches,
    #[sea_orm(
        belongs_to = "super::bots::Entity",
        from = "Column::BotId",
        to = "super::bots::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Bots,
}

impl Related<super::matches::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Matches.def()
    }
}

impl Related<super::bots::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bots.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
