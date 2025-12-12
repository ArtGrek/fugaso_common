use std::sync::Arc;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use async_trait::async_trait;
use essential_data::sequence_generator::{GeneratorDispatcher, LegacyHiloGenerator};
use crate::sequence_generator::{IdGenerator};
#[cfg(feature = "redis")]
use redis_macros::{FromRedisValue, ToRedisArgs};
#[cfg(feature = "redis")]
use essential_data::active_store::{ActiveStore, StoreValue};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "common_round")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[async_trait]
impl<G: IdGenerator + Send + Sync> GeneratorDispatcher<G> for Model {
    async fn call(s: &G) -> Result<i64, DbErr> {
        s.gen_common_round().await
    }
}

pub fn create_sequence(pool: Arc<DatabaseConnection>) -> LegacyHiloGenerator {
    LegacyHiloGenerator::new(
        "common_round_sequence",
        49,
        pool,
    )
}

#[cfg(feature = "redis")]
#[derive(Debug, DeriveIntoActiveModel, Default, Clone, Serialize, Deserialize, FromRedisValue, ToRedisArgs)]
pub struct StoreModel {
    #[serde(skip_serializing_if = "StoreValue::is_none", default)]
    pub id: StoreValue<i64>,
}

#[cfg(feature = "redis")]
impl ActiveStore for ActiveModel {
    type R = StoreModel;

    fn to_active_store(self) -> Self::R {
        Self::R {
            id: self.id.into(),
        }
    }
}