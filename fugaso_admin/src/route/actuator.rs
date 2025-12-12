use salvo::{handler, Response, Router};
use salvo::http::header::CONTENT_TYPE;
use salvo::http::HeaderValue;
use salvo::prelude::Json;
use serde::{Deserialize, Serialize};

pub const END_ROOT: &'static str = "health";

#[derive(Serialize, Deserialize)]
pub enum Status {
    UP
}

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: Status,
}

pub fn create() -> Router {
    Router::with_path(END_ROOT).get(health)
}

#[handler]
pub async fn health(res: &mut Response) -> Result<(), salvo::Error> {
    res.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    res.render(Json(HealthResponse {
        status: Status::UP
    }));
    Ok(())
}
