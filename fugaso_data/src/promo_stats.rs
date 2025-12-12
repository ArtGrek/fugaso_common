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
#[sea_orm(table_name = "promo_stats")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    #[sea_orm(unique)]
    pub user_id: i64,
    #[sea_orm(column_name = "totalout")]
    pub total_out: i64,
    #[sea_orm(column_name = "optlock")]
    pub opt_lock: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
    belongs_to = "essential_data::user_user::Entity",
    from = "Column::UserId",
    to = "essential_data::user_user::Column::Id",
    on_update = "NoAction",
    on_delete = "NoAction"
    )]
    User,
}

impl Related<essential_data::user_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
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
        s.gen_promo_stats().await
    }
}

pub fn create_sequence(pool: Arc<DatabaseConnection>) -> LegacyHiloGenerator {
    LegacyHiloGenerator::new(
        "promo_stats_sequence",
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
    pub user_id: StoreValue<i64>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub total_out: StoreValue<i64>,
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub opt_lock: StoreValue<i32>,
}

#[cfg(feature = "redis")]
impl ActiveStore for ActiveModel {
    type R = StoreModel;

    fn to_active_store(self) -> Self::R {
        StoreModel{
            id: self.id.into(),
            user_id: self.user_id.into(),
            total_out: self.total_out.into(),
            opt_lock: self.opt_lock.into(),
        }
    }
}