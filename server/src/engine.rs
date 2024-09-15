use anyhow::{anyhow, Context};
use rand::{seq::SliceRandom, Rng};
use sea_orm::prelude::TimeDateTimeWithTimeZone;
use sea_orm::FromQueryResult;
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, JoinType,
    QueryFilter, QueryOrder, QuerySelect, QueryTrait, RelationTrait, Set, TransactionTrait,
};
use sea_query::Expr;
use serde::{Deserialize, Serialize};

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use proglad_controller::{manager, match_runner};
use proglad_db as db;

struct DbMatchData {
    game: db::games::Model,
    game_program: db::programs::Model,
    bots: Vec<db::bots::Model>,
    bot_programs: Vec<db::programs::Model>,
}

#[derive(Debug)]
pub struct MyDbError {
    #[allow(dead_code)]
    pub context: String,
    #[allow(dead_code)]
    pub db_error: DbErr,
}

impl std::fmt::Display for MyDbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MyDbError {}

pub async fn ensure_compiled(
    man: &manager::Manager,
    db: &DatabaseConnection,
    program_id: i64,
) -> anyhow::Result<()> {
    let cached = man.is_program_cached(program_id).await;
    let Some(program) = db::programs::Entity::find_by_id(program_id)
        .one(db)
        .await
        .context(format!(
            "Failed to fetch program {program_id} from the database"
        ))?
    else {
        return Err(anyhow!("No such program: {program_id}"));
    };
    if cached && program.status == db::programs::Status::CompilationSucceeded {
        return Ok(());
    }
    if cached {
        log::warn!(
            "Program {program_id} is in cache but not marked compiled in the database: status={:?}, {:?}; recompiling.",
            program.status,
            program.status_reason,
        );
    }
    if program.status == db::programs::Status::CompilationSucceeded {
        log::warn!(
            "Program {program_id} is marked compiled in the database but is absent in the cache; recompiling.",
        );
    }
    compile_impl(man, db, program).await
}

pub async fn choose_and_compile_program(db: &DatabaseConnection, man: &manager::Manager) -> bool {
    let Ok(programs) = db::programs::Entity::find()
        .filter(db::programs::Column::Status.eq(db::programs::Status::New))
        .order_by_asc(db::programs::Column::StatusUpdateTime)
        .limit(1)
        .all(db)
        .await
        .inspect_err(|e| log::error!("Failed to query for program: {e:?}"))
    else {
        return false;
    };
    let Some(program) = programs.into_iter().next() else {
        return false;
    };
    let _ = compile_impl(man, db, program).await;
    true
}

pub async fn run_match(
    man: Arc<manager::Manager>,
    db: &DatabaseConnection,
    bots: &[i64],
    config: &match_runner::Config,
) -> anyhow::Result<()> {
    // If this dies, the match gets cancelled. That is OK for now. In the
    // future we could make intermediate results of the matches persist
    // locally on controller(=worker) nodes and then back-populate them
    // into the main DB with RPCs.
    // For now the controller is integrated with the server, and it dying
    // also results in match being cancelled.
    let data = db_fetch_data(db, bots).await?;
    let match_id = db_prepare_match(db, &data).await?;

    let mut agents = Vec::with_capacity(1 + bots.len());
    agents.push(manager::Agent {
        id: data.game_program.id,
        language: from_db_language(data.game_program.language),
        param: [make_param(&data), "inlinevisualize".to_owned()].join(" "),
    });
    for p in data.bot_programs.into_iter() {
        let agent = manager::Agent {
            id: p.id,
            language: from_db_language(p.language),
            param: "".to_owned(),
        };
        agents.push(agent);
    }

    // TODO: parallel compilation
    for a in agents.iter() {
        ensure_compiled(&man, db, a.id).await?;
    }
    // TODO: manage configuration properly.
    let config = manager::MatchConfig {
        config: config.clone(),
        id: match_id,
        agents,
    };

    log::info!("Starting match {match_id}");

    let match_result = manager::run_match(man.clone(), config)
        .await
        .context(format!("Match {match_id} failed to start"))?;
    let score_deltas = match_result.result.as_ref().ok().map(|mr| {
        bots.iter()
            .copied()
            .zip(mr.scores.iter().copied())
            .collect::<Vec<_>>()
    });
    let num_bots = bots.len();
    let ret = match_result.result.as_ref().map_err(|e| anyhow!("{e:?}")).map(|_| ());
    db.transaction(|txn| {
        Box::pin(async move {
            let _ = db_update_match_result(txn, match_id, num_bots, match_result)
                .await
                .context("Failed to update match result")
                .inspect_err(|e| {
                    log::error!("{e:?}");
                });
            if let Some(score_deltas) = score_deltas {
                let _ = db_update_stats_for_match(txn, match_id, score_deltas)
                    .await
                    .inspect_err(|e| {
                        log::error!("{e:?}");
                    });
            }
            Ok::<(), MyDbError>(())
        })
    })
    .await
    .context("Transaction failed")?;
    log::info!("Match {match_id} result: {ret:?}");
    ret
}

async fn select_active_game(db: &DatabaseConnection) -> anyhow::Result<i64> {
    let active_games: Vec<(i64, Option<TimeDateTimeWithTimeZone>)> = db::games::Entity::find()
        .filter(db::games::Column::Status.eq(db::games::Status::Active))
        .join(JoinType::LeftJoin, db::games::Relation::Matches.def())
        .select_only()
        .column(db::games::Column::Id)
        .column_as(db::matches::Column::EndTime.max(), "max_time")
        .group_by(db::games::Column::Id)
        .order_by_asc(sea_query::Expr::cust("max_time"))
        .limit(1)
        .into_tuple()
        .all(db)
        .await
        .context("select_active_game: Failed to fetch active games")?;
    let Some((game_id, _)) = active_games.into_iter().next() else {
        return Err(anyhow!("No active games"));
    };
    Ok(game_id)
}

pub async fn choose_and_run_match(
    db: &DatabaseConnection,
    man: Arc<manager::Manager>,
    config: &match_runner::Config,
) -> anyhow::Result<()> {
    let selected_players = choose_match(db).await?;
    run_match(man, db, &selected_players, config).await
}

fn pick_num_players(game: &db::games::Model, available_players: usize) -> anyhow::Result<usize> {
    if game.min_players > game.max_players {
        return Err(anyhow!(
            "Game {} had {} = min_players > max_players = {}",
            game.id,
            game.min_players,
            game.max_players
        ));
    }
    if (available_players as i32) < game.min_players {
        return Err(anyhow!("Not enough active players for game {}", game.id));
    }
    let num_players = rand::thread_rng()
        .gen_range(game.min_players..=game.max_players.min(available_players as i32));
    if num_players < 0 {
        return Err(anyhow!(
            "Something is terribly wrong : num_palyers={num_players} < 0"
        ));
    }
    if num_players == 0 {
        log::warn!(
            "Unusual: num_players=0. Check game config for game {}",
            game.id
        );
    }
    Ok(num_players as usize)
}

async fn choose_match(db: &DatabaseConnection) -> anyhow::Result<Vec<i64>> {
    // Scaling NOTE: everything is done in one go, assuming all games, bots and matches can be
    // processed at once. When hitting scaling issues, this should be split per-game, as
    // those are completely independent. Within each game, if there are too many bots, those
    // can be split either by leagues or rooms. Leagues are probably preferred, as we
    // want similarly strong bots to be paired against each other.

    let game_id = select_active_game(db).await?;
    let Some(game) = db::games::Entity::find_by_id(game_id)
        .one(db)
        .await
        .context("choose_and_run_match: Failed to read game data for game {game_id}")?
    else {
        return Err(anyhow!(
            "There is no game with id {game_id} which was selected as active"
        ));
    };

    log::info!("Selected game {game_id} to run next match");
    let active_bots = db::bots::Entity::find()
        .filter(
            Condition::all()
                .add(db::bots::Column::GameId.eq(game_id))
                .add(db::bots::Column::SystemStatus.eq(db::bots::SystemStatus::Ok))
                .add(db::bots::Column::OwnerSetStatus.eq(db::bots::OwnerSetStatus::Active)),
        )
        .all(db)
        .await
        .context("choose_and_run_match: Failed to find active bots: {e:?}")?;
    let mut active_bot_ids: Vec<i64> = active_bots.iter().map(|b| b.id).collect();
    let participations = db::match_participations::Entity::find()
        .filter(db::match_participations::Column::BotId.is_in(active_bot_ids.iter().copied()))
        .all(db)
        .await
        .context("choose_and_run_match: Failed to read match participations")?;
    let mut matches = HashMap::<i64, HashSet<i64>>::new();
    for p in participations.iter() {
        matches.entry(p.match_id).or_default().insert(p.bot_id);
    }
    let mut botset_counts: HashMap<Vec<i64>, usize> = Default::default();
    for (_, bs) in matches.into_iter() {
        let mut bs = bs.into_iter().collect::<Vec<_>>();
        bs.sort();
        *botset_counts.entry(bs).or_default() += 1;
    }
    let num_players = pick_num_players(&game, active_bots.len())?;
    let mut selected_players = HashSet::<i64>::new();
    let mut rng = rand::thread_rng();
    for _ in 0..num_players {
        let mut bot_match_counts = HashMap::<i64, usize>::new();
        for (botset, count) in botset_counts.iter() {
            if botset
                .iter()
                .filter(|id| selected_players.contains(id))
                .count()
                != selected_players.len()
            {
                continue;
            }
            for id in botset.iter() {
                if !selected_players.contains(id) {
                    *bot_match_counts.entry(*id).or_default() += count;
                }
            }
        }
        let min_match_count: usize = active_bot_ids
            .iter()
            .map(|id| bot_match_counts.get(id).copied().unwrap_or_default())
            .min()
            .unwrap();
        let candidates = active_bot_ids
            .iter()
            .filter_map(|id| {
                if bot_match_counts.get(id).copied().unwrap_or_default() == min_match_count {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let player = candidates.choose(&mut rng).copied().unwrap();
        active_bot_ids.retain(|id| *id != player);
        selected_players.insert(player);
    }
    let mut selected_players = selected_players.into_iter().collect::<Vec<_>>();
    selected_players.shuffle(&mut rng);
    log::info!("Will run game {game_id} with players {selected_players:?}");
    Ok(selected_players)
}

pub async fn create_bot<C: ConnectionTrait>(
    db: &C,
    game_id: i64,
    owner_id: i64,
    source_path: impl AsRef<Path>,
    language: db::programs::Language,
    name: &str,
) -> anyhow::Result<i64> {
    let source_code = tokio::fs::read_to_string(source_path)
        .await
        .context("Failed to read source tempfile")?;
    let now = TimeDateTimeWithTimeZone::now_utc();
    let program = db::programs::ActiveModel {
        language: Set(language),
        source_code: Set(Some(source_code)),
        status: Set(db::programs::Status::New),
        status_update_time: Set(now),
        ..Default::default()
    };
    let program_id = db::programs::Entity::insert(program)
        .exec(db)
        .await
        .context("Failed to insert program by account {owner_id} for game {game_id}")?
        .last_insert_id;

    let bot = db::bots::ActiveModel {
        name: Set(name.to_owned()),
        owner_id: Set(owner_id),
        program_id: Set(program_id),
        game_id: Set(game_id),
        owner_set_status: Set(db::bots::OwnerSetStatus::Active),
        system_status: Set(db::bots::SystemStatus::Unknown),
        creation_time: Set(now),
        status_update_time: Set(now),
        ..Default::default()
    };
    let bot_id = db::bots::Entity::insert(bot)
        .exec(db)
        .await
        .context("Failed to create bot by account {account_id} for game {game_id}")?
        .last_insert_id;
    Ok(bot_id)
}

fn from_db_language(language: db::programs::Language) -> manager::Language {
    match language {
        db::programs::Language::Cpp => manager::Language::Cpp,
        db::programs::Language::Rust => manager::Language::Rust,
        db::programs::Language::Python => manager::Language::Python,
        db::programs::Language::Go => manager::Language::Go,
        db::programs::Language::Java => manager::Language::Java,
    }
}

async fn db_fetch_data<C: ConnectionTrait>(db: &C, bots: &[i64]) -> anyhow::Result<DbMatchData> {
    // TODO: snapshot transaction?
    // TODO: avoid fetching source code.
    let q = db::prelude::Bots::find()
        .find_also_related(db::prelude::Programs)
        .filter(Expr::col((db::bots::Entity, db::bots::Column::Id)).is_in(bots.iter().cloned()));
    log::debug!("Bots query {}", q.build(db.get_database_backend()));
    let bots_with_programs = q
        .all(db)
        .await
        .context("Failed to select bots from the db")?;
    let missing_programs = bots_with_programs
        .iter()
        .filter_map(|(b, p)| if p.is_some() { None } else { Some(b.id) })
        .collect::<Vec<_>>();
    if !missing_programs.is_empty() {
        return Err(anyhow!(
            "Some bots have missing programs: {missing_programs:?}"
        ));
    }
    let game_id = bots_with_programs[0].0.game_id;
    if bots_with_programs.iter().any(|(b, _)| b.game_id != game_id) {
        return Err(anyhow!(
            "Different game ids found: {:?}",
            bots_with_programs
                .iter()
                .map(|(b, _)| format!("{} -> {}", b.id, b.game_id))
                .collect::<Vec<_>>()
        ));
    }

    let Some((game, Some(game_program))) = db::games::Entity::find_by_id(game_id)
        .find_also_related(db::programs::Entity)
        .one(db)
        .await
        .context(format!("Failed to fetch game {game_id} from db"))?
    else {
        return Err(anyhow!(format!(
            "Game {game_id} or its program is not found."
        )));
    };
    let bots_with_programs = bots_with_programs
        .into_iter()
        .map(|(b, p)| (b.id, (b, p)))
        .collect::<HashMap<_, _>>();
    let mut bot_programs = Vec::with_capacity(bots.len());
    let mut db_bots = Vec::with_capacity(bots.len());
    for id in bots {
        let Some((b, p)) = bots_with_programs.get(id) else {
            return Err(anyhow!("Non-existent bot id: {id}"));
        };
        // TODO: avoid cloning as much.
        bot_programs.push(p.as_ref().unwrap().clone());
        db_bots.push(b.clone());
    }
    Ok(DbMatchData {
        game,
        game_program,
        bots: db_bots,
        bot_programs,
    })
}

async fn db_prepare_match<C: ConnectionTrait>(
    db: &C,
    data: &DbMatchData,
) -> anyhow::Result<manager::MatchId> {
    let m = db::matches::ActiveModel {
        game_id: Set(data.game.id),
        creation_time: Set(TimeDateTimeWithTimeZone::now_utc()),
        system_message: Set("Just created".to_owned()),
        ..Default::default()
    };
    let match_id = db::matches::Entity::insert(m)
        .exec(db)
        .await
        .context("Failed to create a new match")?
        .last_insert_id;
    let participations =
        data.bots
            .iter()
            .enumerate()
            .map(|(i, b)| db::match_participations::ActiveModel {
                bot_id: Set(b.id),
                match_id: Set(match_id),
                ingame_player: Set(1 + i as u32),
                ..Default::default()
            });
    db::match_participations::Entity::insert_many(participations)
        .exec(db)
        .await
        .context(format!(
            "Failed to create match participations for match {match_id}"
        ))?;
    Ok(match_id)
}

async fn db_update_match_result<C: ConnectionTrait>(
    db: &C,
    match_id: manager::MatchId,
    num_players: usize,
    result: manager::FullMatchResult,
) -> anyhow::Result<()> {
    let (matches_update, participations_updates) =
        match_update_from_result(match_id, num_players, result).await;
    let _ = db::matches::Entity::update(matches_update)
        .exec(db)
        .await
        .context(format!(
            "Failed to update match {match_id} with post-match info"
        ))
        .inspect_err(|e| {
            log::error!("{e:?}");
        });
    for (i, p) in participations_updates.into_iter().enumerate() {
        let _ = db::match_participations::Entity::update(p)
            .exec(db)
            .await
            .context(format!(
                "Failed to update match {match_id} participation {}",
                i + 1
            ))
            .inspect_err(|e| {
                log::error!("{e:?}");
            });
    }
    Ok(())
}

async fn db_mark_bots_of_program<C: ConnectionTrait>(
    db: &C,
    program_id: i64,
    status: db::bots::SystemStatus,
) -> Result<(), DbErr> {
    let writeback = db::bots::ActiveModel {
        system_status: Set(status),
        system_status_reason: Set(None),
        status_update_time: Set(TimeDateTimeWithTimeZone::now_utc()),
        ..Default::default()
    };
    db::bots::Entity::update_many()
        .set(writeback)
        .filter(db::bots::Column::ProgramId.eq(program_id))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn db_update_stats_for_match<C: ConnectionTrait>(
    db: &C,
    match_id: i64,
    score_deltas: Vec<(i64, f64)>,
) -> Result<(), MyDbError> {
    let bot_ids = score_deltas.iter().map(|(id, _)| *id);
    let now = TimeDateTimeWithTimeZone::now_utc();
    let stats = db::stats_history::Entity::find()
        .filter(
            Condition::all()
                .add(db::stats_history::Column::Latest.eq(true))
                .add(db::stats_history::Column::BotId.is_in(bot_ids)),
        )
        .all(db)
        .await
        .map_err(|db_error| MyDbError {
            db_error,
            context: "Failed to fetch initial scores from DB".to_owned(),
        })?
        .into_iter()
        .map(|st| (st.bot_id, st))
        .collect::<HashMap<i64, db::stats_history::Model>>();
    let stats_ids = stats.values().map(|s| s.id);
    let update_non_latest = db::stats_history::ActiveModel {
        latest: Set(false),
        ..Default::default()
    };
    db::stats_history::Entity::update_many()
        .set(update_non_latest)
        .filter(db::stats_history::Column::Id.is_in(stats_ids))
        .exec(db)
        .await
        .map_err(|db_error| MyDbError {
            db_error,
            context: "Failed to update non-latest stats".to_owned(),
        })?;
    let new_stats = score_deltas.into_iter().map(|(id, delta)| {
        let mut new_st = db::stats_history::ActiveModel {
            bot_id: Set(id),
            update_time: Set(now),
            match_id: Set(Some(match_id)),
            latest: Set(true),
            ..Default::default()
        };
        match stats.get(&id) {
            Some(st) => {
                new_st.total_score = Set(st.total_score + delta);
                new_st.total_matches = Set(st.total_matches + 1);
            }
            None => {
                new_st.total_score = Set(delta);
                new_st.total_matches = Set(1);
            }
        }
        new_st
    });
    db::stats_history::Entity::insert_many(new_stats)
        .exec(db)
        .await
        .map_err(|db_error| MyDbError {
            db_error,
            context: "Failed to insert new stats".to_owned(),
        })?;
    Ok(())
}

async fn match_update_from_result(
    match_id: manager::MatchId,
    num_players: usize,
    result: manager::FullMatchResult,
) -> (
    db::matches::ActiveModel,
    Vec<db::match_participations::ActiveModel>,
) {
    let mut mu = db::matches::ActiveModel {
        id: Set(match_id),
        start_time: Set(result.start_time),
        end_time: Set(result.end_time),
        ..Default::default()
    };
    mu.log = Set(result.log.as_ref().ok().cloned());
    let mut participations = (1..=num_players)
        .map(|i| db::match_participations::ActiveModel {
            match_id: Set(match_id),
            ingame_player: Set(i as u32),
            ..Default::default()
        })
        .collect::<Vec<_>>();

    match &result.result {
        Ok(cr) => {
            mu.system_message = Set(format!("Complete ({})", cr.reason));
            for (i, score) in cr.scores.iter().enumerate() {
                participations.get_mut(i).map(|p| {
                        p.score = Set(Some(*score));
                    }).unwrap_or_else(|| {
                        log::error!("Match {match_id} returned more scores ({i}) than players ({num_players})");
                    });
            }
            let mut errors = vec![vec![]; num_players];
            for (player, err) in cr.errors.iter() {
                if *player == 0 {
                    log::error!("Match {match_id} return errors for player 0");
                    continue;
                }
                errors.get_mut(player - 1).map(|s| s.push(err.clone())).unwrap_or_else(|| {
                        log::error!("Match {match_id} return errors for non-existent player ({player}); num_players={num_players}");
                    });
            }
            for (i, err) in errors.iter().enumerate() {
                if err.is_empty() {
                    continue;
                }
                participations
                    .get_mut(i)
                    .map(|p| {
                        p.system_message = Set(Some(err.join("\n")));
                    })
                    .unwrap_or_else(|| {
                        log::error!("Match {match_id} has errors for non-existent id {i}")
                    });
            }
        }
        Err(e) => mu.system_message = Set(format!("{e:?}")),
    }
    (mu, participations)
}

fn make_param(data: &DbMatchData) -> String {
    data.game.param.clone().map_or("".to_owned(), |a| {
        a.replace("{num_players}", &format!("{}", data.bots.len()))
    })
}

async fn compile_impl<C: ConnectionTrait + TransactionTrait>(
    man: &manager::Manager,
    db: &C,
    program: db::programs::Model,
) -> anyhow::Result<()> {
    let writeback = db::programs::ActiveModel {
        id: Set(program.id),
        status: Set(db::programs::Status::Compiling),
        status_update_time: Set(TimeDateTimeWithTimeZone::now_utc()),
        ..Default::default()
    };
    db::programs::Entity::update(writeback)
        .exec(db)
        .await
        .context(format!(
            "Failed to write back compilation status for program {}",
            program.id
        ))?;
    let source_code = program
        .source_code
        .ok_or_else(|| anyhow!("Program {} has no source code", program.id))?;
    let compilation_status = man
        .compile(manager::Program {
            id: program.id,
            language: from_db_language(program.language),
            source_code,
        })
        .await;
    let (status, status_reason) = match &compilation_status {
        Ok(()) => (db::programs::Status::CompilationSucceeded, None),
        Err(e) => (
            db::programs::Status::CompilationFailed,
            Some(format!("{e:?}")),
        ),
    };
    let bot_status = match status {
        db::programs::Status::CompilationSucceeded => db::bots::SystemStatus::Ok,
        db::programs::Status::CompilationFailed => db::bots::SystemStatus::Deactivated,
        _ => db::bots::SystemStatus::Unknown,
    };
    db.transaction(|txn| {
        let id = program.id;
        Box::pin(async move {
            let writeback = db::programs::ActiveModel {
                id: Set(id),
                status: Set(status),
                status_reason: Set(status_reason),
                status_update_time: Set(TimeDateTimeWithTimeZone::now_utc()),
                ..Default::default()
            };
            db::programs::Entity::update(writeback).exec(txn).await?;
            db_mark_bots_of_program(txn, program.id, bot_status).await
        })
    })
    .await?;
    compilation_status
}

#[derive(FromQueryResult)]
struct IdResult {
    id: i64,
}

#[derive(FromQueryResult, Debug)]
struct TimeResult {
    time: Option<TimeDateTimeWithTimeZone>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CleanupConfig {
    pub keep_matches_per_game: u64,
    pub max_delete_matches_num: u64,
}

// Retains 1000 most recent matches per game and
// deletes everything else.
pub async fn cleanup_matches_batch<C: ConnectionTrait>(
    db: &C,
    config: &CleanupConfig,
) -> Result<(), MyDbError> {
    log::info!("Starting stale match cleanup.");
    // TODO: consider querying only active games.
    let game_ids = db::games::Entity::find()
        .select_only()
        .column(db::games::Column::Id)
        .into_model::<IdResult>()
        .all(db)
        .await
        .map_err(|e| MyDbError {
            context: "Failed to query all games".to_owned(),
            db_error: e,
        })?;
    let sql = r#"
        SELECT MIN(end_time) as time FROM (
            SELECT end_time FROM matches
            WHERE game_id = ? AND end_time IS NOT NULL
            ORDER BY end_time DESC
            LIMIT ?
        )
    "#;
    for IdResult { id: game_id } in game_ids {
        log::info!("Cleaning up matches of game {game_id}.");
        let stmt = sea_orm::Statement::from_sql_and_values(
            db.get_database_backend(),
            sql,
            [game_id.into(), config.keep_matches_per_game.into()],
        );
        let threshold_query_result = TimeResult::find_by_statement(stmt)
            .one(db)
            .await
            .map_err(|e| MyDbError {
                context: format!(
                    "Failed to determine the cleanup end time threshold for game {game_id}"
                ),
                db_error: e,
            })?;
        let Some(TimeResult{ time: Some(threshold) }) = threshold_query_result else {
            log::info!("No matches found for game {game_id}, skipping cleanup.");
            continue;
        };
        log::trace!("Match staleness threshold = {threshold:?} for game {game_id}.");
        let ids = db::matches::Entity::find()
            .filter(
                Condition::all()
                    .add(db::matches::Column::EndTime.lt(threshold))
                    .add(db::matches::Column::GameId.eq(game_id)),
            )
            .limit(config.max_delete_matches_num)
            .all(db)
            .await
            .map_err(|e| MyDbError {
                context: format!("Failed to query for matches below threshold for game {game_id}"),
                db_error: e,
            })?;
        log::trace!("Found {} matches to cleanup for game {game_id}.", ids.len());
        if ids.is_empty() {
            continue;
        }
        let res = db::matches::Entity::delete_many()
            .filter(db::matches::Column::Id.is_in(ids.into_iter().map(|idr| idr.id)))
            .exec(db)
            .await
            .map_err(|e| MyDbError {
                context: "Failed to delete matches of game {game_id}".to_owned(),
                db_error: e,
            })?;
        log::info!("Deleted {} matches for game {game_id}", res.rows_affected);
    }
    Ok(())
}
