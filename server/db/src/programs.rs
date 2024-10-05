use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum Status {
    #[sea_orm(string_value = "new")]
    New,
    #[sea_orm(string_value = "compiling")]
    Compiling,
    #[sea_orm(string_value = "succeded")]
    CompilationSucceeded,
    #[sea_orm(string_value = "failed")]
    CompilationFailed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum Language {
    #[sea_orm(string_value = "cpp")]
    Cpp,
    #[sea_orm(string_value = "rust")]
    Rust,
    #[sea_orm(string_value = "python")]
    Python,
    #[sea_orm(string_value = "go")]
    Go,
    #[sea_orm(string_value = "java")]
    Java
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Cpp => "C++",
            Language::Rust => "Rust",
            Language::Python => "Python",
            Language::Go => "Go",
            Language::Java => "Java",
        }
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "programs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub language: Language,
    #[sea_orm(default_value = "new")]
    pub status: Status,
    pub status_reason: Option<String>,
    pub status_update_time: TimeDateTimeWithTimeZone,
    pub is_public: Option<bool>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::bots::Entity")]
    Bots,
    #[sea_orm(has_many = "super::games::Entity")]
    Games,
}

impl Related<super::bots::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Bots.def()
    }
}

impl Related<super::games::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Games.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
