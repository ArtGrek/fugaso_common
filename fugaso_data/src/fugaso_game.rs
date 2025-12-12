use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Default, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "fugaso_game")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    #[sea_orm(unique, column_name = "displayname")]
    pub display_name: Option<String>,
    #[sea_orm(unique, column_name = "gamename")]
    pub game_name: Option<String>,
    #[sea_orm(column_name = "mathclass")]
    pub math_class: String,
    pub origin: String,
    pub promo: bool,
    pub icon_url: String,
    pub mobile_icon_url: String,
    pub background_url: String,
    pub rank: i32,
    pub kind: String,
    pub exposure: i32,
    pub tour_theme: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}