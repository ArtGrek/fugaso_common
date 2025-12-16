use fugaso_data::fugaso_game;
use sea_orm::{ConnectionTrait, EntityTrait, InsertResult, IntoActiveModel};

pub async fn insert_games<C: ConnectionTrait>(db: &C) -> Result<InsertResult<fugaso_data::fugaso_game::ActiveModel>, sea_orm::DbErr> {
    let games = vec![fugaso_game::Model {
        id: 44,
        display_name: Some("Thunder Express".to_string()),
        game_name: Some("thunderexpress".to_string()),
        math_class: "ThunderExpressMath".to_string(),
        origin: "LOCAL".to_string(),
        promo: true,
        ..Default::default()
    }];
    fugaso_game::Entity::insert_many(games.into_iter().map(|m| m.into_active_model())).exec(db).await
}
