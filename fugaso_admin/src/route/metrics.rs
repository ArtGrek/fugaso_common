use std::marker::PhantomData;

use crate::config::ServerConfig;
use crate::dispatcher::IDispacthercontext;
use fugaso_core::protocol;
use fugaso_math::protocol::{ReSpinInfo, RestoreInfo};
use salvo::http::header::CONTENT_TYPE;
use salvo::http::HeaderValue;
use salvo::prelude::{Json, StatusError};
use salvo::{handler, Depot, Response, Router};

use super::options::options_handle;

pub const END_ROOT: &str = "metrics";
pub const END_ONLINE: &str = "online";
pub const END_STATE: &str = "state";

pub fn create<D: IDispacthercontext + Send + Sync + 'static>() -> Router {
    Router::with_path(END_ROOT)
        .push(
            Router::with_path(END_ONLINE)
                .get(Online::<D> {
                    phantom: PhantomData,
                })
                .options(options_handle),
        )
        .push(
            Router::with_path(END_STATE)
                .get(State::<D> {
                    phantom: PhantomData,
                })
                .options(options_handle),
        )
}

pub struct Online<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> Online<D> {
    pub async fn handle(res: &mut Response, depot: &mut Depot) -> Result<(), salvo::Error> {
        let cfg = depot
            .obtain::<ServerConfig<D>>()
            .map_err(|_| StatusError::internal_server_error())?;
        let result = cfg.dispatcher_context.online().await;
        res.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        match result {
            Ok(response) => res.render(Json(response)),
            Err(e) => res.render(Json(protocol::Response::Error::<ReSpinInfo, RestoreInfo>(
                e,
            ))),
        };
        Ok(())
    }
}

pub struct State<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> State<D> {
    pub async fn handle(res: &mut Response, depot: &mut Depot) -> Result<(), salvo::Error> {
        let cfg = depot
            .obtain::<ServerConfig<D>>()
            .map_err(|_| StatusError::internal_server_error())?;
        let result = cfg.dispatcher_context.state().await;
        res.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        match result {
            Ok(response) => res.render(Json(response)),
            Err(e) => res.render(Json(protocol::Response::Error::<ReSpinInfo, RestoreInfo>(
                e,
            ))),
        };
        Ok(())
    }
}
