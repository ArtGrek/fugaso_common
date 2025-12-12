use std::marker::PhantomData;

use salvo::{Depot, handler, Request, Response, Router};
use salvo::__private::tracing::error;
use salvo::http::header::CONTENT_TYPE;
use salvo::http::{HeaderValue, StatusError};
use salvo::prelude::StatusCode;
use crate::config::ServerConfig;
use crate::dispatcher::IDispacthercontext;
use crate::manager::TournamentResult;

pub const END_ROOT: &str = "tournament";
pub const END_HANDLE: &str = "handle";
const SECURE_MAX_SIZE: usize = 1024 * 1024;

pub fn create<D: IDispacthercontext + Send + Sync + 'static>() -> Router {
    Router::with_path(END_ROOT)
        .push(
            Router::with_path(END_HANDLE).post(Handle::<D>{phantom: PhantomData})
        )
}

struct Handle<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> Handle<D> {
    pub async fn handle(req: &mut Request, res: &mut Response, depot: &mut Depot) -> Result<(), salvo::Error> {
        let result = req.parse_body_with_max_size::<TournamentResult>(SECURE_MAX_SIZE).await.map_err(|e| {
            error!("error parse tournament result {e}");
            e
        })?;
        let cfg = depot.obtain::<ServerConfig<D>>().map_err(|_| StatusError::internal_server_error())?;
        match cfg.tour_manager.handle(result).await {
            Ok(e) => { cfg.dispatcher_context.tournament_win(e) }
            Err(e) => { error!("error handle tournament {e}!") }
        };
        res.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
        res.status_code(StatusCode::NO_CONTENT);
        Ok(())
    }
}
