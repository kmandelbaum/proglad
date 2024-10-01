use sea_orm::entity::prelude::*;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i8", db_type = "Integer")]
pub enum Kind {
    #[default]
    Unknown = 0,
    SourceCode = 1,
    StaticContent = 2,
    MatchReplay = 3,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i8", db_type = "Integer")]
pub enum ContentType {
    #[default]
    None = 0,
    PlainText = 1,
    Html = 2,
    Png = 3,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i8", db_type = "Integer")]
pub enum Compression {
    #[default]
    Uncompressed = 0,
    Gzip = 1,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "i8", db_type = "Integer")]
pub enum OwningEntity {
    #[default]
    None = 0,
    Account = 1,
    Game = 2,
    Match = 3,
    Program = 4,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "files")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub owning_entity: OwningEntity,
    pub owning_id: Option<i64>,
    pub name: String,

    #[sea_orm(indexed)]
    pub last_update: TimeDateTimeWithTimeZone,
    pub content_type: ContentType,
    pub kind: Kind,
    pub compression: Compression,

    // We might want to query everything except content.
    // In which case we'll need an option to skip this column.
    pub content: Option<Vec<u8>>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            id: 0,
            owning_entity: OwningEntity::None,
            owning_id: None,
            name: String::new(),
            last_update: TimeDateTimeWithTimeZone::now_utc(),
            content_type: ContentType::None,
            kind: Kind::Unknown,
            compression: Compression::Uncompressed,
            content: None,
        }
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation { }

impl ActiveModelBehavior for ActiveModel {}
