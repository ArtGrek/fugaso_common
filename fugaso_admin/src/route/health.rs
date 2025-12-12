use salvo::{handler, Response, Router};
use salvo::http::header::CONTENT_TYPE;
use salvo::http::HeaderValue;
use salvo::prelude::Json;
use serde::{Deserialize, Serialize};

use super::options::options_handle;

pub const END_ROOT: &str = "health";

pub fn create() -> Router {
    Router::with_path(END_ROOT).get(health).options(options_handle)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Health {
    UP
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: Health,
}

#[handler]
pub async fn health(res: &mut Response) -> Result<(), salvo::Error> {
    res.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    res.render(Json(HealthResponse {
        status: Health::UP,
    }));
    Ok(())
}