use sea_orm::{DeriveActiveEnum, EnumIter};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i8", db_type = "Integer")]
pub enum EntityKind {
    #[default]
    None = 0,
    Account = 1,
    Game = 2,
    Match = 3,
    Program = 4,
    Bot = 5,
}
