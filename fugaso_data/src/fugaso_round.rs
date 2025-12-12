use std::sync::Arc;
use async_trait::async_trait;
use essential_data::sequence_generator::{GeneratorDispatcher, LegacyHiloGenerator};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, IntoActiveValue, Unchanged};
use serde::{Deserialize, Serialize};
use crate::model::ActiveClone;
use crate::sequence_generator::IdGenerator;
#[cfg(feature = "redis")]
use redis_macros::{FromRedisValue, ToRedisArgs};
#[cfg(feature = "redis")]
use essential_data::active_store::{ActiveStore, StoreValue};

#[derive(Debug, PartialEq, Eq, Hash, Clone, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(Some(15))")]
#[allow(non_camel_case_types)]
pub enum RoundStatus {
    #[sea_orm(string_value = "SUCCESS")]
    SUCCESS,
    #[sea_orm(string_value = "REMOTE_ERROR")]
    REMOTE_ERROR,
    #[sea_orm(string_value = "ROLLBACK")]
    ROLLBACK,
    #[sea_orm(string_value = "DECLINE")]
    DECLINE,
    #[sea_orm(string_value = "CUTBACK")]
    CUTBACK,
    #[sea_orm(string_value = "CUTBACK_ERROR")]
    CUTBACK_ERROR,
    #[sea_orm(string_value = "MANUAL_CLOSE")]
    MANUAL_CLOSE,
    #[sea_orm(string_value = "FORCE_CLOSE")]
    FORCE_CLOSE,
    #[sea_orm(string_value = "FORCE_CUTBACK")]
    FORCE_CUTBACK,
    #[sea_orm(string_value = "ADMIN_CLOSE")]
    ADMIN_CLOSE,
}

impl IntoActiveValue<RoundStatus> for RoundStatus {
    fn into_active_value(self) -> ActiveValue<RoundStatus> {
        ActiveValue::Set(self)
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, EnumIter, DeriveActiveEnum, Default, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "String(Some(15))")]
#[allow(non_camel_case_types)]
pub enum RoundDetail {
    #[default]
    #[sea_orm(string_value = "SIMPLE")]
    SIMPLE,
    #[sea_orm(string_value = "RICH")]
    RICH,
}

impl IntoActiveValue<RoundDetail> for RoundDetail {
    fn into_active_value(self) -> ActiveValue<RoundDetail> {
        ActiveValue::Set(self)
    }
}

impl RoundDetail {
    pub fn alias(&self) -> &str {
        match self {
            RoundDetail::SIMPLE => { "simple" }
            RoundDetail::RICH => { "rich" }
        }
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Default)]
#[sea_orm(table_name = "game_scale_round")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub bet: i32,
    pub line: i32,
    #[sea_orm(column_name = "timestampclose")]
    pub timestamp_close: Option<DateTime>,
    #[sea_orm(column_name = "timestampopen")]
    pub timestamp_open: Option<DateTime>,
    pub game_id: Option<i64>,
    pub user_id: Option<i64>,
    pub denom: i32,
    #[sea_orm(column_type = "Decimal(Some((19, 2)))", nullable)]
    pub balance: Option<Decimal>,
    pub reels: Option<i32>,
    pub status: Option<RoundStatus>,
    pub multi: i32,
    pub detail: RoundDetail,
    #[sea_orm(unique)]
    pub common_id: Option<i64>,
    pub bet_counter: i32,
    pub stake: Option<i64>,
    pub win: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
    belongs_to = "super::common_round::Entity",
    from = "Column::CommonId",
    to = "super::common_round::Column::Id",
    on_update = "NoAction",
    on_delete = "NoAction"
    )]
    CommonRound,
    #[sea_orm(
    belongs_to = "super::fugaso_game::Entity",
    from = "Column::GameId",
    to = "super::fugaso_game::Column::Id",
    on_update = "NoAction",
    on_delete = "NoAction"
    )]
    FugasoGame,
    #[sea_orm(
    belongs_to = "essential_data::user_user::Entity",
    from = "Column::UserId",
    to = "essential_data::user_user::Column::Id",
    on_update = "NoAction",
    on_delete = "NoAction"
    )]
    UserUser,
    #[sea_orm(has_many = "super::fugaso_action::Entity")]
    Action,
}

impl Related<super::common_round::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CommonRound.def()
    }
}

impl Related<super::fugaso_game::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FugasoGame.def()
    }
}

impl Related<essential_data::user_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UserUser.def()
    }
}

impl Related<super::fugaso_action::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Action.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl<G: IdGenerator + Send + Sync> GeneratorDispatcher<G> for Model {
    async fn call(s: &G) -> Result<i64, DbErr> {
        s.gen_round().await
    }
}

pub fn create_sequence(pool: Arc<DatabaseConnection>) -> LegacyHiloGenerator {
    LegacyHiloGenerator::new(
        "game_scale_round_sequence",
        49,
        pool,
    )
}

impl ActiveClone<ActiveModel> for Model {
    fn clone_active_model(&self) -> ActiveModel {
        ActiveModel {
            id: Unchanged(self.id),
            bet: Unchanged(self.bet),
            line: Unchanged(self.line),
            timestamp_close: Unchanged(self.timestamp_close),
            timestamp_open: Unchanged(self.timestamp_open),
            game_id: Unchanged(self.game_id),
            user_id: Unchanged(self.user_id),
            denom: Unchanged(self.denom),
            balance: Unchanged(self.balance),
            reels: Unchanged(self.reels),
            status: Unchanged(self.status.clone()),
            multi: Unchanged(self.multi),
            detail: Unchanged(self.detail.clone()),
            common_id: Unchanged(self.common_id),
            bet_counter: Unchanged(self.bet_counter),
            stake: Unchanged(self.stake),
            win: Unchanged(self.win),
        }
    }

    fn unchanged_active_model() -> ActiveModel {
        ActiveModel {
            id: Unchanged(0),
            bet: Unchanged(0),
            line: Unchanged(0),
            timestamp_close: Unchanged(None),
            timestamp_open: Unchanged(None),
            game_id: Unchanged(None),
            user_id: Unchanged(None),
            denom: Unchanged(0),
            balance: Unchanged(None),
            reels: Unchanged(None),
            status: Unchanged(None),
            multi: Unchanged(0),
            detail: Unchanged(RoundDetail::SIMPLE),
            common_id: Unchanged(None),
            bet_counter: Unchanged(1),
            stake: Unchanged(None),
            win: Unchanged(None),
        }
    }
}

#[cfg(feature = "redis")]
#[derive(Debug, DeriveIntoActiveModel, Default, Clone, Serialize, Deserialize, FromRedisValue, ToRedisArgs)]
pub struct StoreModel {
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub id: StoreValue<i64>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub bet: StoreValue<i32>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub line: StoreValue<i32>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub timestamp_close: StoreValue<Option<DateTime>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub timestamp_open: StoreValue<Option<DateTime>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub game_id: StoreValue<Option<i64>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub user_id: StoreValue<Option<i64>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub denom: StoreValue<i32>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub balance: StoreValue<Option<Decimal>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub reels: StoreValue<Option<i32>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub status: StoreValue<Option<RoundStatus>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub multi: StoreValue<i32>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub detail: StoreValue<RoundDetail>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub common_id: StoreValue<Option<i64>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub bet_counter: StoreValue<i32>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub stake: StoreValue<Option<i64>>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub win: StoreValue<Option<i64>>,
}

#[cfg(feature = "redis")]
impl ActiveStore for ActiveModel {
    type R = StoreModel;

    fn to_active_store(self) -> Self::R {
        Self::R {
            id: self.id.into(),
            bet: self.bet.into(),
            line: self.line.into(),
            timestamp_close: self.timestamp_close.into(),
            timestamp_open: self.timestamp_open.into(),
            game_id: self.game_id.into(),
            user_id: self.user_id.into(),
            denom: self.denom.into(),
            balance: self.balance.into(),
            reels: self.reels.into(),
            status: self.status.into(),
            multi: self.multi.into(),
            detail: self.detail.into(),
            common_id: self.common_id.into(),
            bet_counter: self.bet_counter.into(),
            stake: self.stake.into(),
            win: self.win.into(),
        }
    }
}