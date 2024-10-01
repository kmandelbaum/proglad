use proglad_db::{accounts, bots, games, prelude::*, programs};
use sea_orm::entity::prelude::TimeDateTimeWithTimeZone;
use sea_orm::{EntityTrait, Set};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

fn idx<E: EntityTrait>(s: &sea_orm::Schema, e: E) -> Vec<IndexCreateStatement> {
    s.create_index_from_entity(e)
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let s = sea_orm::Schema::new(m.get_database_backend());
        m.create_table(s.create_table_from_entity(Accounts)).await?;
        m.create_table(s.create_table_from_entity(Programs)).await?;
        m.create_table(s.create_table_from_entity(Games)).await?;
        m.create_table(s.create_table_from_entity(Bots)).await?;
        m.create_table(s.create_table_from_entity(Matches)).await?;
        m.create_table(s.create_table_from_entity(MatchParticipations))
            .await?;
        let s = &s;
        let all_idx = [
            idx(s, Accounts),
            idx(s, Programs),
            idx(s, Games),
            idx(s, Bots),
            idx(s, Matches),
            idx(s, MatchParticipations),
        ]
        .into_iter()
        .flatten();
        for i in all_idx {
            m.create_index(i).await?;
        }
        if std::env::var("PROGLAD_POPULATE_DATABASE").is_ok() {
            populate_database(m).await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(
            Table::drop()
                .table(MatchParticipations)
                .if_exists()
                .to_owned(),
        )
        .await
        .inspect_err(log_err("drop match_participations"))?;
        m.drop_table(Table::drop().table(Matches).if_exists().to_owned())
            .await
            .inspect_err(log_err("drop matches"))?;
        m.drop_table(Table::drop().table(Bots).if_exists().to_owned())
            .await
            .inspect_err(log_err("drop bots"))?;
        m.drop_table(Table::drop().table(Games).if_exists().to_owned())
            .await
            .inspect_err(log_err("drop games"))?;
        m.drop_table(Table::drop().table(Programs).if_exists().to_owned())
            .await
            .inspect_err(log_err("drop programs"))?;
        m.drop_table(Table::drop().table(Accounts).if_exists().to_owned())
            .await
            .inspect_err(log_err("drop accounts"))?;
        Ok(())
    }
}

fn log_err<'a>(ctx: &'a str) -> impl FnOnce(&DbErr) + 'a {
    move |e| {
        eprintln!("{ctx}: {e}");
    }
}

async fn populate_database<'a>(m: &'a SchemaManager<'a>) -> Result<(), DbErr> {
    let db = m.get_connection();
    let account = accounts::ActiveModel {
        name: Set("km".to_owned()),
        email: Set(Some("submulticativity@gmail.com".to_owned())),
        ..Default::default()
    };
    let account_id = accounts::Entity::insert(account)
        .exec(db)
        .await
        .map_err(|e| DbErr::Custom(format!("{e}")))?
        .last_insert_id;
    populate_database_lowest_unique(db, account_id).await?;
    populate_database_halma_quad(db, account_id).await?;
    //populate_database_halma_hex(db, account_id).await?;
    Ok(())
}

async fn populate_database_halma_quad<'a, C: ConnectionTrait>(
    db: &C,
    account_id: i64,
) -> Result<(), DbErr> {
    let gameserver_path = "../games/halma-quad/server/main.rs";
    let now = TimeDateTimeWithTimeZone::now_utc();
    let source_code = tokio::fs::read_to_string(gameserver_path)
        .await
        .map_err(|e| {
            DbErr::Custom(format!(
                "Failed to read halma-quad game server file {gameserver_path} for database seeding: {e}"
            ))
        })?;
    let game_program = programs::ActiveModel {
        language: Set(programs::Language::Rust),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        ..Default::default()
    };
    let game_program_id = programs::Entity::insert(game_program)
        .exec(db)
        .await?
        .last_insert_id;
    let bot_path = "../games/halma-quad/player-greedy/main.rs";
    let source_code = tokio::fs::read_to_string(bot_path).await.map_err(|e| {
        DbErr::Custom(format!(
            "Failed to read greedy bot file {gameserver_path} for database seeding: {e}"
        ))
    })?;
    let bot_program = programs::ActiveModel {
        language: Set(programs::Language::Rust),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        is_public: Set(Some(true)),
        ..Default::default()
    };
    let bot1_program_id = programs::Entity::insert(bot_program.clone())
        .exec(db)
        .await?
        .last_insert_id;
    let bot2_program_id = programs::Entity::insert(bot_program)
        .exec(db)
        .await?
        .last_insert_id;

    let bot_path = "../games/halma-quad/player-basic/basic.py";
    let source_code = tokio::fs::read_to_string(bot_path).await.map_err(|e| {
        DbErr::Custom(format!(
            "Failed to read basic bot file {gameserver_path} for database seeding: {e}"
        ))
    })?;
    let bot_program = programs::ActiveModel {
        language: Set(programs::Language::Python),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        is_public: Set(Some(true)),
        ..Default::default()
    };
    let bot3_program_id = programs::Entity::insert(bot_program.clone())
        .exec(db)
        .await?
        .last_insert_id;

    let game = games::ActiveModel {
        name: Set("halma-quad".to_owned()),
        description: Set("Halma game on a 16x16 grid. \
            Move your pieces home across the field from where you start before your opponent does. \
            Pieces move and jump like in checkers but are not taken off the board."
            .to_owned()),
        program_id: Set(game_program_id),
        status: Set(games::Status::Active),
        min_players: Set(2),
        max_players: Set(2),
        ..Default::default()
    };
    let game_id = games::Entity::insert(game)
        .exec(db)
        .await
        .map_err(|e| DbErr::Custom(format!("{e}")))?
        .last_insert_id;

    let bot1 = bots::ActiveModel {
        name: Set("halma-quad-greedy-1".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot1_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        is_reference_bot: Set(Some(true)),
        ..Default::default()
    };
    let bot2 = bots::ActiveModel {
        name: Set("halma-quad-greedy-2".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot2_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        ..Default::default()
    };
    let bot3 = bots::ActiveModel {
        name: Set("halma-quad-greedy-3".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot2_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        ..Default::default()
    };
    let bot4 = bots::ActiveModel {
        name: Set("halma-quad-greedy-4".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot2_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        ..Default::default()
    };
    let bot5 = bots::ActiveModel {
        name: Set("halma-quad-basic-1".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot3_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        is_reference_bot: Set(Some(true)),
        ..Default::default()
    };
    //let bot4 = bots::ActiveModel {
    //    name: Set("halma-quad-basic-2".to_owned()),
    //    game_id: Set(game_id),
    //    owner_id: Set(account_id),
    //    program_id: Set(bot3_program_id),
    //    owner_set_status: Set(bots::OwnerSetStatus::Active),
    //    system_status: Set(bots::SystemStatus::Unknown),
    //    creation_time: Set(now),
    //    status_update_time: Set(now),
    //    ..Default::default()
    //};
    bots::Entity::insert(bot1).exec(db).await?;
    bots::Entity::insert(bot2).exec(db).await?;
    bots::Entity::insert(bot3).exec(db).await?;
    bots::Entity::insert(bot4).exec(db).await?;
    bots::Entity::insert(bot5).exec(db).await?;
    Ok(())
}

#[allow(dead_code)]
async fn populate_database_halma_hex<'a, C: ConnectionTrait>(
    db: &C,
    account_id: i64,
) -> Result<(), DbErr> {
    let gameserver_path = "../games/halma-hex/server/main.rs";
    let now = TimeDateTimeWithTimeZone::now_utc();
    let source_code = tokio::fs::read_to_string(gameserver_path)
        .await
        .map_err(|e| {
            DbErr::Custom(format!(
                "Failed to read game server file {gameserver_path} for database seeding: {e}"
            ))
        })?;
    let game_program = programs::ActiveModel {
        language: Set(programs::Language::Rust),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        ..Default::default()
    };
    let game_program_id = programs::Entity::insert(game_program)
        .exec(db)
        .await?
        .last_insert_id;
    let bot_path = "../games/halma-hex/player-greedy/main.rs";
    let source_code = tokio::fs::read_to_string(bot_path).await.map_err(|e| {
        DbErr::Custom(format!(
            "Failed to read greedy bot file {gameserver_path} for database seeding: {e}"
        ))
    })?;
    let bot_program = programs::ActiveModel {
        language: Set(programs::Language::Rust),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        is_public: Set(Some(true)),
        ..Default::default()
    };
    let bot1_program_id = programs::Entity::insert(bot_program.clone())
        .exec(db)
        .await?
        .last_insert_id;
    let bot2_program_id = programs::Entity::insert(bot_program)
        .exec(db)
        .await?
        .last_insert_id;

    let bot_path = "../games/halma-hex/player-basic/basic.py";
    let source_code = tokio::fs::read_to_string(bot_path).await.map_err(|e| {
        DbErr::Custom(format!(
            "Failed to read halma-hex basic bot file {gameserver_path} for database seeding: {e}"
        ))
    })?;
    let bot_program = programs::ActiveModel {
        language: Set(programs::Language::Python),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        is_public: Set(Some(true)),
        ..Default::default()
    };
    let bot3_program_id = programs::Entity::insert(bot_program.clone())
        .exec(db)
        .await?
        .last_insert_id;

    let game = games::ActiveModel {
        name: Set("halma-hex".to_owned()),
        description: Set("Halma game on a hexagonal grid. \
            Move your pieces home across the field from where you start before your opponent does. \
            Pieces move and jump like in checkers but are not taken off the board."
            .to_owned()),
        program_id: Set(game_program_id),
        status: Set(games::Status::Active),
        min_players: Set(2),
        max_players: Set(2),
        ..Default::default()
    };
    let game_id = games::Entity::insert(game)
        .exec(db)
        .await
        .map_err(|e| DbErr::Custom(format!("{e}")))?
        .last_insert_id;

    let bot1 = bots::ActiveModel {
        name: Set("halma-hex-greedy-1".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot1_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        is_reference_bot: Set(Some(true)),
        ..Default::default()
    };
    let bot2 = bots::ActiveModel {
        name: Set("halma-hex-greedy-2".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot2_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        ..Default::default()
    };
    let bot3 = bots::ActiveModel {
        name: Set("halma-hex-basic".to_owned()),
        game_id: Set(game_id),
        owner_id: Set(account_id),
        program_id: Set(bot3_program_id),
        owner_set_status: Set(bots::OwnerSetStatus::Active),
        system_status: Set(bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        is_reference_bot: Set(Some(true)),
        ..Default::default()
    };
    bots::Entity::insert(bot1).exec(db).await?;
    bots::Entity::insert(bot2).exec(db).await?;
    bots::Entity::insert(bot3).exec(db).await?;
    Ok(())
}

async fn populate_database_lowest_unique<'a, C: ConnectionTrait>(
    db: &C,
    account_id: i64,
) -> Result<(), DbErr> {
    let gameserver_path = "../games/lowest-unique/server/main.rs";
    let source_code = tokio::fs::read_to_string(gameserver_path)
        .await
        .map_err(|e| {
            DbErr::Custom(format!(
                "Failed to read game server file {gameserver_path} for database seeding: {e}"
            ))
        })?;
    let now = TimeDateTimeWithTimeZone::now_utc();
    let game_program = programs::ActiveModel {
        language: Set(programs::Language::Rust),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        ..Default::default()
    };
    let game_program_id = programs::Entity::insert(game_program)
        .exec(db)
        .await?
        .last_insert_id;
    let bot_path = "../games/lowest-unique/player-random/main.go";
    let source_code = tokio::fs::read_to_string(bot_path).await.map_err(|e| {
        DbErr::Custom(format!(
            "Failed to read greedy bot file {gameserver_path} for database seeding: {e}"
        ))
    })?;
    let bot_program = programs::ActiveModel {
        language: Set(programs::Language::Go),
        source_code: Set(Some(source_code)),
        status: Set(programs::Status::New),
        status_update_time: Set(now),
        is_public: Set(Some(true)),
        ..Default::default()
    };
    let bot1_program_id = programs::Entity::insert(bot_program.clone())
        .exec(db)
        .await?
        .last_insert_id;

    let game = games::ActiveModel {
        name: Set("lowest-unique".to_owned()),
        description: Set("Pick a number. Lowest unique number wins.".to_owned()),
        program_id: Set(game_program_id),
        status: Set(games::Status::Active),
        param: Set(Some("{num_players} 10 500".to_owned())),
        min_players: Set(3),
        max_players: Set(6),
        ..Default::default()
    };
    let game_id = games::Entity::insert(game)
        .exec(db)
        .await
        .map_err(|e| DbErr::Custom(format!("{e}")))?
        .last_insert_id;

    for i in 0..10 {
        let bot = bots::ActiveModel {
            name: Set(format!("lowest-unique-random-{i}")),
            game_id: Set(game_id),
            owner_id: Set(account_id),
            program_id: Set(bot1_program_id),
            owner_set_status: Set(bots::OwnerSetStatus::Active),
            system_status: Set(bots::SystemStatus::Unknown),
            creation_time: Set(now),
            status_update_time: Set(now),
            is_reference_bot: Set(Some(i == 0)),
            ..Default::default()
        };
        bots::Entity::insert(bot).exec(db).await?;
    }
    Ok(())
}
