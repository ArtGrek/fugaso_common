use std::sync::Arc;
use async_trait::async_trait;
use essential_data::sequence_generator::{GeneratorDispatcher, LegacyHiloGenerator};
use essential_data::version::{VersionedActiveModel, VersionToColumn};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use crate::sequence_generator::IdGenerator;
#[cfg(feature = "redis")]
use redis_macros::{FromRedisValue, ToRedisArgs};
#[cfg(feature = "redis")]
use essential_data::active_store::{ActiveStore, StoreValue};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "promo_acccount")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub balance: i32,
    #[sea_orm(column_name = "optlock")]
    pub opt_lock: i32,
    pub game_id: Option<i64>,
    pub user_id: Option<i64>,
    pub multi: i32,
    pub active: bool,
    pub expire: Option<DateTime>,
    #[sea_orm(unique)]
    pub offer_id: Option<Uuid>,
    pub charge_id: Option<String>,
    pub stake: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
    belongs_to = "super::fugaso_game::Entity",
    from = "Column::GameId",
    to = "super::fugaso_game::Column::Id",
    on_update = "NoAction",
    on_delete = "NoAction"
    )]
    Game,
    #[sea_orm(
    belongs_to = "essential_data::user_user::Entity",
    from = "Column::UserId",
    to = "essential_data::user_user::Column::Id",
    on_update = "NoAction",
    on_delete = "NoAction"
    )]
    User,
    #[sea_orm(has_many = "super::promo_transaction::Entity")]
    Transaction,
}

impl Related<super::fugaso_game::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Game.def()
    }
}

impl Related<essential_data::user_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl Related<super::promo_transaction::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Transaction.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

impl VersionToColumn for ActiveModel {
    fn ver_column(&self) -> <Self::Entity as EntityTrait>::Column {
        Column::OptLock
    }
}

impl VersionedActiveModel for ActiveModel {}

#[async_trait]
impl<G: IdGenerator + Send + Sync> GeneratorDispatcher<G> for Model {
    async fn call(s: &G) -> Result<i64, DbErr> {
        s.gen_promo_account().await
    }
}

pub fn create_sequence(pool: Arc<DatabaseConnection>) -> LegacyHiloGenerator {
    LegacyHiloGenerator::new(
        "promo_account_sequence",
        1,
        pool,
    )
}

#[cfg(feature = "redis")]
#[derive(Debug, DeriveIntoActiveModel, Default, Clone, Serialize, Deserialize, FromRedisValue, ToRedisArgs)]
pub struct StoreModel {
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub id: StoreValue<i64>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub balance: StoreValue<i32>,
    #[sea_orm(column_name = "optlock")]
    pub opt_lock: StoreValue<i32>,
    pub game_id: StoreValue<Option<i64>>,
    pub user_id: StoreValue<Option<i64>>,
    pub multi: StoreValue<i32>,
    pub active: StoreValue<bool>,
    pub expire: StoreValue<Option<DateTime>>,
    #[sea_orm(unique)]
    pub offer_id: StoreValue<Option<Uuid>>,
    pub charge_id: StoreValue<Option<String>>,
    pub stake: StoreValue<i64>,
}

#[cfg(feature = "redis")]
impl ActiveStore for ActiveModel {
    type R = StoreModel;

    fn to_active_store(self) -> Self::R {
        StoreModel{
            id: self.id.into(),
            balance: self.balance.into(),
            opt_lock: self.opt_lock.into(),
            game_id: self.game_id.into(),
            user_id: self.user_id.into(),
            multi: self.multi.into(),
            active: self.active.into(),
            expire: self.expire.into(),
            offer_id: self.offer_id.into(),
            charge_id: self.charge_id.into(),
            stake: self.stake.into(),
        }
    }
}