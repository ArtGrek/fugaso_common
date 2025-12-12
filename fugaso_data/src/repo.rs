use crate::fugaso_round::RoundStatus;
use crate::{fugaso_action, fugaso_game, fugaso_percent, fugaso_round, launch_info, promo_account, promo_stats, promo_transaction, tournament_gain};
use fugaso_game::Model as Game;
use sea_orm::prelude::Uuid;
use sea_orm::sea_query::Query;
use sea_orm::{ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, DbBackend, DbErr, FromQueryResult, Order, QueryFilter, Statement, TransactionTrait};
use sea_orm::{EntityTrait, QueryOrder, QuerySelect};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
pub struct GameRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl GameRepository {
    pub async fn find_by_name(&self, name: &str) -> Result<Option<Game>, DbErr> {
        fugaso_game::Entity::find().filter(fugaso_game::Column::GameName.eq(name)).one(self.conn.as_ref()).await
    }

    pub async fn find_by_id(&self, id: i64) -> Result<Option<Game>, DbErr> {
        fugaso_game::Entity::find_by_id(id).one(self.conn.as_ref()).await
    }
}

#[derive(Debug)]
pub struct PercentRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl PercentRepository {
    pub async fn find_recursive_percent(&self, user_id: i64, game_id: i64) -> Result<Option<fugaso_percent::Model>, DbErr> {
        let percent = fugaso_percent::Entity::find()
            .from_raw_sql(Statement::from_sql_and_values(
                DbBackend::Postgres,
                r#"with recursive subordinates as (select user_user.id, user_user.operator_id, user_user.usertype
                                from user_user
                                where user_user.id=$1
                                union all
                                select e.id, e.operator_id, e.usertype
                                from user_user as e
                                         join subordinates on e.id = subordinates.operator_id)
select p.id,p.active,p.freepercent,p.menuorder,p.percent,p.possbets,p.game_id,p.operator_id,p.denomination,p.bet_multiplier
from subordinates
         inner join game_scale_percent p on p.operator_id = subordinates.id and p.game_id=$2 limit 1"#,
                vec![user_id.into(), game_id.into()],
            ))
            .one(self.conn.as_ref())
            .await;
        percent
    }
}

#[derive(Debug)]
pub struct RoundRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl RoundRepository {
    pub async fn insert(&self, r: fugaso_round::ActiveModel, a: fugaso_action::ActiveModel) -> Result<(fugaso_round::Model, fugaso_action::Model), DbErr> {
        let tx = self.conn.begin().await?;
        let round = r.insert(&tx).await?;
        let action = a.insert(&tx).await?;

        tx.commit().await?;
        Ok((round, action))
    }

    pub async fn update(&self, r: fugaso_round::ActiveModel, actions: Vec<fugaso_action::ActiveModel>) -> Result<(fugaso_round::Model, Vec<fugaso_action::Model>), DbErr> {
        let tx = self.conn.begin().await?;
        let result = r.update(&tx).await?;
        let mut actions_on = vec![];
        for a in actions {
            let action_on = a.update(&tx).await?;
            actions_on.push(action_on);
        }
        tx.commit().await?;
        Ok((result, actions_on))
    }

    pub async fn find_last_round(&self, user_id: i64, game_id: i64, status: RoundStatus) -> Result<Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>, DbErr> {
        let mut max: Vec<Option<i64>> = fugaso_round::Entity::find()
            .select_only()
            .column(fugaso_round::Column::Id)
            .filter(
                Condition::all().add(fugaso_round::Column::UserId.eq(user_id)).add(fugaso_round::Column::GameId.eq(game_id)).add(fugaso_round::Column::Status.eq(status.clone())),
            )
            .order_by_desc(fugaso_round::Column::Id)
            .limit(1)
            .into_tuple()
            .all(self.conn.as_ref())
            .await?;
        let mut round_with_actions: Vec<(fugaso_round::Model, Vec<fugaso_action::Model>)> = if let Some(Some(m)) = max.pop() {
            fugaso_round::Entity::find().find_with_related(fugaso_action::Entity).filter(fugaso_round::Column::Id.eq(m)).all(self.conn.as_ref()).await?
        } else {
            vec![]
        };

        if let Some(mut r) = round_with_actions.pop() {
            r.1.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }

    pub async fn find_last_rounds_by_status(
        &self,
        user_id: i64,
        game_id: i64,
        status: Vec<RoundStatus>,
    ) -> Result<Option<(fugaso_round::Model, Vec<fugaso_action::Model>)>, DbErr> {
        let mut max: Vec<Option<i64>> = fugaso_round::Entity::find()
            .select_only()
            .column_as(fugaso_round::Column::Id.max(), "max_id")
            .filter(Condition::all().add(fugaso_round::Column::UserId.eq(user_id)).add(fugaso_round::Column::GameId.eq(game_id)).add(fugaso_round::Column::Status.is_in(status)))
            .into_tuple()
            .all(self.conn.as_ref())
            .await?;

        let mut round_with_actions: Vec<(fugaso_round::Model, Vec<fugaso_action::Model>)> = if let Some(Some(m)) = max.pop() {
            fugaso_round::Entity::find().find_with_related(fugaso_action::Entity).filter(fugaso_round::Column::Id.eq(m)).all(self.conn.as_ref()).await?
        } else {
            vec![]
        };
        if let Some(mut r) = round_with_actions.pop() {
            r.1.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(Some(r))
        } else {
            Ok(None)
        }
    }

    pub async fn find_last_rounds(&self, user_id: i64, game_id: i64, limit: u64) -> Result<Vec<(fugaso_round::Model, Vec<fugaso_action::Model>)>, DbErr> {
        /*let round_with_actions: Vec<(fugaso_round::Model, Vec<fugaso_action::Model>)> = fugaso_round::Entity::find()
        .find_with_related(fugaso_action::Entity)
        .filter(
            fugaso_round::Column::Id.in_subquery(
                Query::select()
                    .column(fugaso_round::Column::Id)
                    .cond_where(Condition::all().add(fugaso_round::Column::UserId.eq(user_id)).add(fugaso_round::Column::GameId.eq(game_id)))
                    .from(fugaso_round::Entity)
                    .order_by(fugaso_round::Column::TimestampOpen, Order::Desc)
                    .limit(limit)
                    .to_owned(),
            ),
        )
        .all(self.conn.as_ref())
        .await?; slow beacuse or order by game_scale_round.id*/
        let rows: Vec<(fugaso_round::Model, Option<fugaso_action::Model>)> = fugaso_round::Entity::find()
            .find_also_related(fugaso_action::Entity)
            .filter(
                fugaso_round::Column::Id.in_subquery(
                    Query::select()
                        .column(fugaso_round::Column::Id)
                        .cond_where(Condition::all().add(fugaso_round::Column::UserId.eq(user_id)).add(fugaso_round::Column::GameId.eq(game_id)))
                        .from(fugaso_round::Entity)
                        .order_by(fugaso_round::Column::TimestampOpen, Order::Desc)
                        .limit(limit)
                        .to_owned(),
                ),
            )
            .all(self.conn.as_ref())
            .await?;
        let map: HashMap<i64, (fugaso_round::Model, Vec<fugaso_action::Model>)> = rows.into_iter().fold(HashMap::new(), |mut acc, v| {
            if let Some(p) = acc.get_mut(&v.0.id) {
                if let Some(a) = v.1 {
                    p.1.push(a);
                }
            } else {
                let acts = v.1.map(|a| vec![a]).unwrap_or(vec![]);
                acc.insert(v.0.id, (v.0, acts));
            }
            acc
        });
        Ok(map.into_values().collect())
    }

    pub async fn find_round_finished(&self, id: i64) -> Result<Vec<(fugaso_round::Model, Vec<fugaso_action::Model>)>, DbErr> {
        let round_with_actions: Vec<(fugaso_round::Model, Vec<fugaso_action::Model>)> = fugaso_round::Entity::find()
            .find_with_related(fugaso_action::Entity)
            .filter(
                Condition::all().add(fugaso_round::Column::CommonId.eq(id)).add(fugaso_round::Column::TimestampClose.is_not_null()), //.add(fugaso_round::Column::Status.eq(RoundStatus::SUCCESS))
            )
            .all(self.conn.as_ref())
            .await?;
        Ok(round_with_actions)
    }
}

pub struct TournamentGainRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl TournamentGainRepository {
    pub async fn find_gains(&self, inbound_ids: Vec<Uuid>) -> Result<Vec<tournament_gain::Model>, DbErr> {
        tournament_gain::Entity::find().filter(tournament_gain::Column::InboundId.is_in(inbound_ids)).all(self.conn.as_ref()).await
    }

    pub async fn find_gains_by_rounds<T: FromQueryResult>(&self, round_ids: Vec<String>) -> Result<Vec<T>, DbErr> {
        tournament_gain::Entity::find()
            .select_only()
            .column(tournament_gain::Column::Id)
            .column(tournament_gain::Column::Tour)
            .column(tournament_gain::Column::Amount)
            .column(tournament_gain::Column::RoundId)
            .column(tournament_gain::Column::TimeDone)
            .column(tournament_gain::Column::Place)
            .filter(tournament_gain::Column::RoundId.is_in(round_ids))
            .order_by(tournament_gain::Column::TimeDone, Order::Desc)
            .into_model::<T>()
            .all(self.conn.as_ref())
            .await
    }
}

pub struct PromoAccountRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl PromoAccountRepository {
    pub async fn find_account(&self, user_id: i64, game_id: i64) -> Result<Option<promo_account::Model>, DbErr> {
        promo_account::Entity::find()
            .filter(Condition::all().add(promo_account::Column::UserId.eq(user_id)).add(promo_account::Column::GameId.eq(game_id)))
            .one(self.conn.as_ref())
            .await
    }
}

pub struct PromoStatsRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl PromoStatsRepository {
    pub async fn find(&self, user_id: i64) -> Result<Option<promo_stats::Model>, DbErr> {
        promo_stats::Entity::find().filter(promo_stats::Column::UserId.eq(user_id)).one(self.conn.as_ref()).await
    }
}

pub struct PromoTranRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl PromoTranRepository {
    pub async fn find_transactions(&self, common_id: i64) -> Result<Vec<promo_transaction::Model>, DbErr> {
        promo_transaction::Entity::find().filter(promo_transaction::Column::RoundId.eq(common_id)).all(self.conn.as_ref()).await
    }
}

#[derive(Debug)]
pub struct LaunchInfoRepository {
    pub conn: Arc<DatabaseConnection>,
}

impl LaunchInfoRepository {
    pub async fn find_all(&self, block: bool) -> Result<Vec<launch_info::Model>, DbErr> {
        launch_info::Entity::find().filter(launch_info::Column::Block.eq(block)).all(self.conn.as_ref()).await
    }
}
