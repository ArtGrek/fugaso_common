use std::collections::HashMap;
use std::sync::Arc;
use chrono::Local;
use essential_core::err_on;
use essential_core::error::ServerError;
use essential_data::repo::SqlAction::Insert;
use essential_data::repo::{BaseRepository, Repository, UserRepository};
use sea_orm::prelude::{Decimal, Uuid};
use sea_orm::IntoActiveModel;
use serde::{Deserialize, Serialize};
use fugaso_core::tournament::{TournamentConfig, TournamentCreateAct, TournamentGainService, TournamentPlace};
use fugaso_data::repo::TournamentGainRepository;
use fugaso_data::sequence_generator::{FugasoIdGenerator, IdGenerator};
use fugaso_data::tournament_gain;
use crate::dispatcher::TournamentEventWin;

#[derive(Debug, Serialize, Deserialize)]
pub struct TournamentResult {
    pub awards: Vec<TournamentAward>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TournamentAward {
    pub id: i64,
    pub amount: Decimal,
    pub user: String,
    pub remote_id: Uuid,
    pub tour: String,
    pub place: i32,
    pub balance: Decimal,
    pub event_id: Uuid,
    pub ip: String,
    pub remote_code: i32,
}

pub struct TournamentManager {
    base_repo: Arc<BaseRepository>,
    user_repo: Arc<UserRepository>,
    gain_repo: Arc<TournamentGainRepository>,
    gain_service: Arc<TournamentGainService>,
    id_gen: Arc<FugasoIdGenerator>,
    ip: Arc<String>,
}

impl TournamentManager {
    pub const RC_NOT_DONE: i32 = -1;
    pub fn new(
        base_repo: Arc<BaseRepository>,
        user_repo: Arc<UserRepository>,
        gain_service: Arc<TournamentGainService>,
        gain_repo: Arc<TournamentGainRepository>,
        id_gen: Arc<FugasoIdGenerator>,
        tour_config: Arc<TournamentConfig>,
    ) -> Self {
        Self {
            base_repo,
            user_repo,
            gain_repo,
            gain_service,
            id_gen,
            ip: tour_config.ip.clone(),
        }
    }

    pub async fn handle(&self, result: TournamentResult) -> Result<TournamentEventWin, ServerError> {
        let winners: HashMap<Uuid, Vec<TournamentPlace>> = result.awards.iter().fold(HashMap::new(), |mut acc, v| {
            if let Some(vec) = acc.get_mut(&v.event_id) {
                vec.push(TournamentPlace { name: v.user.clone(), balance: v.balance });
            } else {
                acc.insert(v.event_id, vec![TournamentPlace { name: v.user.clone(), balance: v.balance }]);
            }
            acc
        });
        let balance_user = result.awards.iter().map(|a| (a.remote_id, (a.event_id, a.balance, a.id))).collect::<HashMap<_, _>>();

        let awards = result.awards.into_iter().filter(|a| a.ip == self.ip.as_str()).collect::<Vec<_>>();
        let fresh = awards.iter()
            .filter(|a| a.remote_code == Self::RC_NOT_DONE)
            .collect::<Vec<_>>();
        let already_in = self.gain_repo.find_gains(fresh.iter().map(|v| v.remote_id).collect()).await.map_err(|e| err_on!(e))?;
        let not_in_db = fresh.iter()
            .filter(|a| !already_in.iter().any(|g| a.remote_id == g.inbound_id))
            .map(|a| *a)
            .collect::<Vec<_>>();
        let names = not_in_db.iter().map(|a| a.user.as_str()).collect::<Vec<_>>();
        let user_rates = self.user_repo.find_user_rates(names, "EUR").await.map_err(|e| err_on!(e))?.into_iter().map(|r| (r.user_name.clone(), r)).collect::<HashMap<_, _>>();
        let mut gains = vec![];
        for a in not_in_db {
            if let Some(r) = user_rates.get(&a.user) {
                let amount = (a.amount / r.rate).round_dp(2);
                let gain = tournament_gain::Model {
                    id: self.id_gen.gen_gain().await.map_err(|e| err_on!(e))?,
                    user_id: r.id,
                    inbound_id: a.remote_id,
                    amount,
                    amount_euro: a.amount,
                    place: a.place,
                    remote_code: Self::RC_NOT_DONE,
                    tour: a.tour.clone(),
                    time_done: Local::now().naive_local(),
                    round_id: Self::RC_NOT_DONE.to_string(),
                    opt_lock: Some(0),
                    ..Default::default()
                };
                gains.push(Insert(gain.into_active_model()));
            }
        }
        let mut saved = self.base_repo.store_vec(gains).await.map_err(|e| err_on!(e))?;

        let not_committed = already_in.iter()
            .filter(|r| r.remote_code != Self::RC_NOT_DONE)
            .map(|t|
                TournamentCreateAct {
                    outbound_id: t.inbound_id,
                    remote_code: t.remote_code,
                }
            )
            .collect::<Vec<_>>();

        self.gain_service.commit_wins(not_committed).await;

        let mut not_performed = already_in.into_iter()
            .filter(|r| r.remote_code == Self::RC_NOT_DONE)
            .collect::<Vec<_>>();

        saved.append(&mut not_performed);
        Ok(TournamentEventWin {
            winners: winners.into_iter().map(|(k, v)| (k, Arc::new(v))).collect::<HashMap<_, _>>(),
            gains: saved,
            balance_user,
        })
    }
}