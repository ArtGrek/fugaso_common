use std::sync::Arc;
use async_trait::async_trait;
use essential_data::sequence_generator::{GeneratorDispatcher, LegacyHiloGenerator};
use essential_data::version::{VersionedActiveModel, VersionToColumn};
use sea_orm::entity::prelude::*;
use crate::sequence_generator::IdGenerator;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Default)]
#[sea_orm(table_name = "tournament_gain")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub user_id: i64,
    #[sea_orm(unique)]
    pub inbound_id: Uuid,
    #[sea_orm(column_type = "Decimal(Some((16, 2)))")]
    pub amount: Decimal,
    #[sea_orm(column_type = "Decimal(Some((16, 2)))")]
    pub amount_euro: Decimal,
    pub place: i32,
    pub remote_code: i32,
    pub tour: String,
    pub time_done: DateTime,
    pub round_id: String,
    pub remote_id: Option<String>,
    pub remote_message: Option<String>,
    pub opt_lock: Option<i32>,
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
        s.gen_gain().await
    }
}

pub fn create_sequence(pool: Arc<DatabaseConnection>) -> LegacyHiloGenerator {
    LegacyHiloGenerator::new(
        "tournament_gain_sequence",
        49,
        pool,
    )
}