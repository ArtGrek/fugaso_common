use super::options::options_handle;
use crate::cache::{Cache, ReqCache};
use crate::config::ServerConfig;
use crate::dispatcher::{IDispacthercontext, ResponseStacked};
use essential_core::account_service::ProxyAlias;
use fugaso_core::protocol::{self, LoginRequest};
use fugaso_math::protocol::{ReSpinInfo, RestoreInfo};
use log::{debug, error};
use salvo::cache::MokaStore;
use salvo::http::header::{CACHE_STATUS, CONTENT_TYPE};
use salvo::http::HeaderValue;
use salvo::prelude::{StatusCode, StatusError};
use salvo::{handler, Depot, Request, Response, Router, Scribe};
use sea_orm::prelude::Uuid;
use std::hash::Hash;
use std::marker::PhantomData;
use std::time::Duration;

pub const REQUEST_ID: &str = "request-id";
pub const AUTH_TOKEN: &str = "auth-token";
pub const END_ROOT: &str = "simplex";
pub const END_HANDLE: &str = "handle";
pub const END_REPLAY: &str = "replay";
pub const END_REPLAY_HANDLE: &str = "{round_id}/handle";
pub const END_REPLAY_PING: &str = "{round_id}/ping";
pub const END_PING: &str = "ping";

pub fn create<D: IDispacthercontext + Send + Sync + 'static>(root: Option<&str>) -> Router {
    Router::with_path(root.unwrap_or(END_ROOT))
        .push(
            Router::with_path(END_HANDLE)
                .post(Handle::<D> {
                    phantom: PhantomData,
                })
                .hoop(create_cache())
                .options(options_handle),
        )
        .push(
            Router::with_path(END_PING)
                .post(Ping::<D> {
                    phantom: PhantomData,
                })
                .options(options_handle),
        )
}

pub fn create_replay<D: IDispacthercontext + Send + Sync + 'static>() -> Router {
    Router::with_path(END_REPLAY)
        .push(
            Router::with_path(END_REPLAY_HANDLE)
                .post(ReplayHandle::<D> {
                    phantom: PhantomData,
                })
                .hoop(create_cache())
                .options(options_handle),
        )
        .push(
            Router::with_path(END_REPLAY_PING)
                .post(Ping::<D> {
                    phantom: PhantomData,
                })
                .options(options_handle),
        )
}

fn create_cache<V: Hash + Eq + Send + Sync + Clone + 'static>() -> Cache<MokaStore<V>, ReqCache> {
    Cache::new(
        MokaStore::builder()
            .time_to_live(Duration::from_secs(15 * 60)) //24h
            .build(),
        ReqCache {},
    )
}

pub struct Ping<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> Ping<D> {
    pub async fn handle(
        req: &mut Request,
        res: &mut Response,
        depot: &mut Depot,
    ) -> Result<(), salvo::Error> {
        let cfg = depot
            .obtain::<ServerConfig<D>>()
            .map_err(|_| StatusError::internal_server_error())?;
        if let Some(h) = req.header::<String>(AUTH_TOKEN) {
            cfg.dispatcher_context.ping(h).await
        };
        res.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        res.status_code(StatusCode::NO_CONTENT);
        Ok(())
    }
}

pub struct Handle<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> Handle<D> {
    pub async fn handle(
        req: &mut Request,
        res: &mut Response,
        depot: &mut Depot,
    ) -> Result<(), salvo::Error> {
        handle_on::<D>(req, res, depot, None).await
    }
}

pub struct ReplayHandle<D: IDispacthercontext + Send + Sync> {
    phantom: PhantomData<D>,
}

#[handler]
impl<D: IDispacthercontext + Send + Sync + 'static> ReplayHandle<D> {
    pub async fn handle(
        req: &mut Request,
        res: &mut Response,
        depot: &mut Depot,
    ) -> Result<(), salvo::Error> {
        let round_id = req
            .param::<i64>("round_id")
            .ok_or_else(|| StatusError::bad_request())?;
        handle_on::<D>(req, res, depot, Some(round_id)).await
    }
}

pub async fn handle_on<D: IDispacthercontext + Send + Sync + 'static>(
    req: &mut Request,
    res: &mut Response,
    depot: &mut Depot,
    round_id: Option<i64>,
) -> Result<(), salvo::Error> {
    let cfg = depot
        .obtain::<ServerConfig<D>>()
        .map_err(|_| StatusError::internal_server_error())?;
    let result = if let Some(h) = req.header::<String>(AUTH_TOKEN) {
        let player_req = req.parse_body::<serde_json::Value>().await?;
        let request_id = req.header::<Uuid>(REQUEST_ID);
        cfg.dispatcher_context
            .send((h, request_id, player_req))
            .await
    } else {
        debug!("login attempt...");
        let mut player_req = req.parse_json::<LoginRequest>().await.map_err(|e| {
            error!("error parse request: {e}");
            e
        })?;

        let ip_address_list = req.header::<String>("x-forwarded-for");
        let user_agent = req.header::<String>("user-agent");

        let round_actions = match round_id {
            Some(id) => {
                let mut rounds = cfg
                    .round_repo
                    .find_round_finished(id)
                    .await
                    .map_err(|_e| StatusError::internal_server_error())?;
                if let Some(mut p) = rounds.pop() {
                    p.1.sort_by_key(|a| a.id);
                    let game_id = p.0.game_id.ok_or_else(|| StatusError::bad_request())?;
                    let game_name = cfg
                        .game_service
                        .get_game_by_id(game_id)
                        .await
                        .map_err(|_e| StatusError::bad_request())?
                        .and_then(|g| g.game_name)
                        .ok_or_else(|| StatusError::bad_request())?;
                    player_req.session.game_name = game_name;
                    player_req.session.mode = ProxyAlias::Demo;
                    Some(p)
                } else {
                    return Err(StatusError::not_found().into());
                }
            }
            None => None,
        };

        let dispatcher = cfg
            .create_dispatcher(&player_req.session.game_name, round_id.is_some())
            .await
            .map_err(|e| {
                error!("{e}");
                StatusError::internal_server_error().detail(e.message)
            })?;
        match cfg
            .dispatcher_context
            .register(
                dispatcher,
                player_req,
                ip_address_list,
                user_agent,
                round_actions,
            )
            .await
        {
            Ok((r, t)) => {
                res.headers_mut().insert(
                    AUTH_TOKEN,
                    HeaderValue::from_str(&t).map_err(|_| StatusError::internal_server_error())?,
                );
                Ok(r)
            }
            Err(e) => Err(e),
        }
    };
    let response = match result {
        Ok(response) => response,
        Err(e) => ResponseStacked {
            id: None,
            answer: Box::new(vec![protocol::Response::Error::<ReSpinInfo, RestoreInfo>(
                e,
            )]),
            cache: false,
        },
    };
    if response.cache {
        res.headers_mut()
            .insert(CACHE_STATUS, HeaderValue::from_static("enable"));
    }
    if let Some(id) = response.id {
        res.headers_mut().insert(
            REQUEST_ID,
            HeaderValue::from_str(&id.to_string())
                .map_err(|_| StatusError::internal_server_error())?,
        );
    }
    res.render(Bytes(response.answer.render()?));
    Ok(())
}
pub struct Bytes(Vec<u8>);

impl Scribe for Bytes {
    fn render(self, res: &mut Response) {
        res.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        res.write_body(self.0).ok();
    }
}
