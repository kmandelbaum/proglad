use crate::handlers::prelude::*;

#[derive(Serialize, Clone, Debug)]
pub struct ParticipationTmplData {
    pub ingame_player: u32,
    pub bot_name: String,
    pub score: String,
    pub highlight: bool,
    pub system_message: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct MatchTmplData {
    pub match_id: i64,
    // TODO: use three stages here - creted, started, finished and show that time instead.
    pub creation_time: String,
    pub game_id: i64,
    pub game_name: String,
    pub participations: Vec<ParticipationTmplData>,
    pub duration: String,
    pub system_message: String,
}

#[derive(Serialize, Clone)]
pub struct LanguageChoice {
    pub name: String,
    pub value: String,
    pub selected: bool,
}

pub async fn match_tmpl_data(
    db: &DatabaseConnection,
    matches: &[db::matches::Model],
    highlight: impl Fn(&db::match_participations::Model) -> bool,
) -> Result<Vec<MatchTmplData>, AppHttpError> {
    let games = db_games(
        db,
        matches
            .iter()
            .map(|m| m.game_id)
            .collect::<HashSet<_>>()
            .into_iter(),
    )
    .await
    .map_err(|e| {
        log::error!("Failed to fetch game names: {e:?}");
        AppHttpError::Internal
    })?;
    let participations = db_participations_in_matches(db, matches.iter().map(|m| m.id))
        .await
        .map_err(|e| {
            log::error!("Failed to fetch match participations for matches of owner: {e:?}");
            AppHttpError::Internal
        })?;
    let bot_owners_and_names = db_bot_owners_and_names(
        db,
        participations
            .iter()
            .map(|p| p.bot_id)
            .collect::<HashSet<_>>(),
    )
    .await
    .map_err(|e| {
        log::error!("Failed to fetch all participating bots: {e:?}");
        AppHttpError::Internal
    })?;
    let bot_names = HashMap::<i64, String>::from_iter(
        bot_owners_and_names
            .into_iter()
            .map(|(id, owner, name)| (id, format!("{owner}/{name}"))),
    );
    let mut matches_data = HashMap::<i64, MatchTmplData>::new();
    let game_names = HashMap::<i64, String>::from_iter(games.into_iter().map(|g| (g.id, g.name)));
    for m in matches {
        let duration = m
            .end_time
            .and_then(|end| m.start_time.map(|start| format_duration(end - start)))
            .unwrap_or_default();
        matches_data.insert(
            m.id,
            MatchTmplData {
                match_id: m.id,
                creation_time: format_time(m.creation_time),
                game_id: m.game_id,
                game_name: game_names.get(&m.game_id).cloned().unwrap_or_default(),
                participations: vec![],
                duration,
                system_message: m.system_message.clone(),
            },
        );
    }
    for p in participations.into_iter() {
        let Some(md) = matches_data.get_mut(&p.match_id) else {
            continue;
        };
        md.participations.push(ParticipationTmplData {
            ingame_player: p.ingame_player,
            bot_name: bot_names.get(&p.bot_id).cloned().unwrap_or_default(),
            highlight: highlight(&p),
            system_message: p.system_message.unwrap_or_default(),
            score: p.score.map_or(String::new(), |s| format!("{s:.2}")),
        });
    }
    for m in matches_data.values_mut() {
        m.participations.sort_by_key(|p| p.ingame_player);
    }
    let mut matches_data = matches_data.into_values().collect::<Vec<_>>();
    matches_data.sort_by(|md1, md2| md1.creation_time.cmp(&md2.creation_time).reverse());
    Ok(matches_data)
}

pub async fn db_games(
    db: &DatabaseConnection,
    ids: impl Iterator<Item = i64>,
) -> Result<Vec<db::games::Model>, DbErr> {
    db::games::Entity::find()
        .filter(db::games::Column::Id.is_in(ids))
        .all(db)
        .await
}

async fn db_participations_in_matches(
    db: &DatabaseConnection,
    match_ids: impl IntoIterator<Item = i64>,
) -> Result<Vec<db::match_participations::Model>, DbErr> {
    db::match_participations::Entity::find()
        .filter(db::match_participations::Column::MatchId.is_in(match_ids))
        .all(db)
        .await
}

async fn db_bot_owners_and_names(
    db: &DatabaseConnection,
    ids: impl IntoIterator<Item = i64>,
) -> Result<Vec<(i64, String, String)>, DbErr> {
    db::bots::Entity::find()
        .left_join(db::accounts::Entity)
        .filter(db::bots::Column::Id.is_in(ids))
        .select_only()
        .column(db::bots::Column::Id)
        .column(db::accounts::Column::Name)
        .column(db::bots::Column::Name)
        .into_tuple()
        .all(db)
        .await
}

pub async fn db_recent_matches(
    db: &DatabaseConnection,
    filter: sea_orm::Condition,
    limit: u64,
) -> Result<Vec<db::matches::Model>, DbErr> {
    db::matches::Entity::find()
        .filter(filter)
        .order_by_desc(db::matches::Column::EndTime)
        .limit(limit)
        .all(db)
        .await
}

pub async fn db_usernames(
    db: &DatabaseConnection,
    ids: impl Iterator<Item = i64>,
) -> Result<HashMap<i64, String>, DbErr> {
    Ok(db::accounts::Entity::find()
        .filter(db::accounts::Column::Id.is_in(ids))
        .all(db)
        .await?
        .into_iter()
        .map(|acc| (acc.id, acc.name))
        .collect())
}

fn format_duration(duration: time::Duration) -> String {
    format!("{:.3}s", duration.as_seconds_f32())
}

pub fn format_time(time: time::OffsetDateTime) -> String {
    let format = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    time.format(&format).unwrap()
}
