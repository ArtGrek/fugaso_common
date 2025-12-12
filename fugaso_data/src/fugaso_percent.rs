use sea_orm::entity::prelude::*;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Default)]
#[sea_orm(table_name = "game_scale_percent")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub active: bool,
    #[sea_orm(column_name = "freepercent")]
    pub free_percent: i32,
    #[sea_orm(column_name = "menuorder")]
    pub menu_order: Option<i32>,
    pub percent: i32,
    #[sea_orm(column_name = "possbets")]
    pub poss_bets: Option<String>,
    pub game_id: i64,
    pub operator_id: i64,
    pub denomination: Option<String>,
    pub bet_multiplier: i32,
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
    FugasoGame,
    #[sea_orm(
        belongs_to = "essential_data::user_user::Entity",
        from = "Column::OperatorId",
        to = "essential_data::user_user::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    User,
}

impl Related<super::fugaso_game::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::FugasoGame.def()
    }
}

impl Related<essential_data::user_user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
