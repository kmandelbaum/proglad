// The business logic of the ACL checking goes here:
use derive_more::Display;
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DbErr, EntityTrait, FromQueryResult, QueryFilter,
    QuerySelect,
};

use db::acls::*;
use proglad_db as db;

#[derive(Clone, Copy)]
pub enum Requester {
    Unauthenticated,
    System,
    Account(i64),
}

#[derive(Debug, Display)]
pub enum Error {
    Denied,
    NotFound(String),
    DbErr(DbErr),
    InvalidArgument(String),
}

impl std::error::Error for Error {}

pub async fn check<C: ConnectionTrait>(
    db: &C,
    requester: Requester,
    access_type: AccessType,
    entity_kind: db::common::EntityKind,
    entity_id: Option<i64>,
) -> Result<(), Error> {
    if let Requester::System = requester {
        return Ok(());
    }
    match (access_type, entity_kind) {
        (AccessType::ReadMatchesOfGame, db::common::EntityKind::Game) => {}
        (AccessType::ReadMatchesOfGame, _) => {
            return Err(Error::InvalidArgument(format!(
                "Access type of ReadMatchesOfGame is specified with entity {entity_kind:?}."
            )));
        }
        (AccessType::CreateBotsInGame, db::common::EntityKind::Game) => {}
        (AccessType::CreateBotsInGame, _) => {
            return Err(Error::InvalidArgument(format!(
                "Access type of CreateBotsInGame is specified with entity {entity_kind:?}."
            )));
        }
        _ => {}
    };
    let requester_clause = {
        let rc = Condition::any().add(Column::GranteeKind.eq(GranteeKind::Everyone));
        if let Requester::Account(id) = requester {
            rc.add(Column::GranteeKind.eq(GranteeKind::AnyRegisteredUser))
                .add(
                    Condition::all()
                        .add(Column::GranteeKind.eq(GranteeKind::Account))
                        .add(Column::GranteeId.eq(Some(id))),
                )
        } else {
            rc
        }
    };
    let entity_clause = {
        // Direct access.
        let ec = Condition::any().add(
            Condition::all()
                .add(Column::EntityKind.eq(entity_kind))
                .add(Column::EntityId.eq(entity_id))
                .add(Column::AccessType.eq(access_type)),
        );
        match (entity_kind, access_type, entity_id) {
            // Inderect access to matches of game.
            (db::common::EntityKind::Match, AccessType::Read, Some(entity_id)) => {
                // Find the game ID first as a separate query.
                // Have to accept that for now.
                // Options to improve are to either translate to SQL,
                // propagate the game_id hint from the caller
                // (but that won't help with replays),
                // or otherwise refactor the whole thing.
                let IdResult { id: game_id } = db::matches::Entity::find_by_id(entity_id)
                    .select_only()
                    .column_as(db::matches::Column::GameId, "id")
                    .into_model::<IdResult>()
                    .one(db)
                    .await
                    .map_err(Error::DbErr)?
                    .ok_or_else(|| Error::NotFound(format!("Match {entity_id} not found.")))?;
                ec.add(
                    Condition::all()
                        .add(Column::EntityKind.eq(db::common::EntityKind::Game))
                        .add(Column::EntityId.eq(Some(game_id)))
                        .add(Column::AccessType.eq(AccessType::ReadMatchesOfGame)),
                )
            }
            _ => ec,
        }
    };
    let c = Condition::all().add(requester_clause).add(entity_clause);
    let acl = Entity::find()
        .filter(c)
        .limit(1)
        .all(db)
        .await
        .map_err(Error::DbErr)?;
    if acl.is_empty() {
        Err(Error::Denied)
    } else {
        Ok(())
    }
}

#[derive(FromQueryResult)]
struct IdResult {
    id: i64,
}
