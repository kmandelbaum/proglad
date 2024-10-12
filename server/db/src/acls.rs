use crate::common;
use sea_orm::entity::prelude::*;
use sea_orm::strum::IntoEnumIterator;
use sea_orm::{Condition, Set};

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum GranteeKind {
    #[sea_orm(string_value = "everyone")]
    Everyone,
    #[sea_orm(string_value = "admin")]
    Admin,
    #[sea_orm(string_value = "account")]
    Account,
    #[sea_orm(string_value = "any-registered-user")]
    AnyRegisteredUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(None)")]
pub enum AccessType {
    #[sea_orm(string_value = "read")]
    Read,
    #[sea_orm(string_value = "write")]
    Write,
    // Allows the grantee to read replays and match metadata of all the
    // matches of a given game. This is equivalent to giving Read access
    // for all the matches.
    #[sea_orm(string_value = "read-matches-of-game")]
    ReadMatchesOfGame,

    #[sea_orm(string_value = "create-bots-in-game")]
    CreateBotsInGame,
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "acls")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    // Must not be None.
    pub entity_kind: common::EntityKind,

    // If None gives access to everything.
    pub entity_id: Option<i64>,

    pub grantee_kind: GranteeKind,
    pub grantee_id: Option<i64>,
    pub access_type: AccessType,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

pub async fn populate_default_acl(
    db: &DatabaseConnection,
    site_admin_account_id: i64,
) -> Result<(), DbErr> {
    let acl = common::EntityKind::iter()
        .filter(|&ek| ek != common::EntityKind::None && ek != common::EntityKind::Match)
        .flat_map(|ek| {
            let base = ActiveModel {
                entity_kind: Set(ek),
                grantee_kind: Set(GranteeKind::Account),
                grantee_id: Set(Some(site_admin_account_id)),
                ..Default::default()
            };
            [
                Some(ActiveModel {
                    access_type: Set(AccessType::Read),
                    ..base.clone()
                }),
                Some(ActiveModel {
                    access_type: Set(AccessType::Write),
                    ..base.clone()
                }),
                if ek == common::EntityKind::Game {
                    Some(ActiveModel {
                        access_type: Set(AccessType::ReadMatchesOfGame),
                        ..base
                    })
                } else {
                    None
                },
            ]
            .into_iter()
            .flatten()
        });
    Entity::insert_many(acl).exec(db).await?;
    Ok(())
}

pub async fn set_game_public<C: ConnectionTrait>(
    db: &C,
    game_id: i64,
    public: bool,
) -> Result<(), DbErr> {
    let base = ActiveModel {
        entity_kind: Set(common::EntityKind::Game),
        entity_id: Set(Some(game_id)),
        grantee_kind: Set(GranteeKind::Everyone),
        ..Default::default()
    };
    if public {
        let acl = [
            ActiveModel {
                access_type: Set(AccessType::Read),
                ..base.clone()
            },
            ActiveModel {
                access_type: Set(AccessType::ReadMatchesOfGame),
                ..base.clone()
            },
            ActiveModel {
                access_type: Set(AccessType::CreateBotsInGame),
                grantee_kind: Set(GranteeKind::AnyRegisteredUser),
                ..base.clone()
            },
        ];
        Entity::insert_many(acl).exec(db).await?;
    } else {
        Entity::delete_many()
            .filter(
                Condition::all()
                    .add(Column::EntityKind.eq(common::EntityKind::Game))
                    .add(Column::EntityId.eq(Some(game_id)))
                    .add(
                        Condition::any()
                            .add(Column::GranteeKind.eq(GranteeKind::Everyone))
                            .add(Column::GranteeKind.eq(GranteeKind::AnyRegisteredUser)),
                    ),
            )
            .exec(db)
            .await?;
    }
    Ok(())
}

pub async fn set_program_public<C: ConnectionTrait>(
    db: &C,
    program_id: i64,
    public: bool,
) -> Result<(), DbErr> {
    if public {
        let entry = ActiveModel {
            access_type: Set(AccessType::Read),
            entity_kind: Set(common::EntityKind::Program),
            entity_id: Set(Some(program_id)),
            grantee_kind: Set(GranteeKind::Everyone),
            ..Default::default()
        };
        Entity::insert(entry).exec(db).await?;
    } else {
        Entity::delete_many()
            .filter(
                Condition::all()
                    .add(Column::AccessType.eq(AccessType::Read))
                    .add(Column::EntityKind.eq(common::EntityKind::Program))
                    .add(Column::EntityId.eq(Some(program_id)))
                    .add(Column::GranteeKind.eq(GranteeKind::Everyone)),
            )
            .exec(db)
            .await?;
    }
    Ok(())
}
