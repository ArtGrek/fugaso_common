use crate::route::client::REQUEST_ID;
use async_trait::async_trait;
use log::error;
use salvo::cache::{CacheIssuer, CacheStore, CachedBody, CachedEntry, MethodSkipper};
use salvo::handler::Skipper;
use salvo::http::header::CACHE_STATUS;
use salvo::{Depot, FlowCtrl, Handler, Request, Response};

pub struct ReqCache {}

impl CacheIssuer for ReqCache {
    type Key = String;

    async fn issue(&self, req: &mut Request, _depot: &Depot) -> Option<Self::Key> {
        req.header::<Self::Key>(REQUEST_ID)
    }
}

pub struct Cache<S, I> {
    store: S,
    issuer: I,
    skipper: Box<dyn Skipper>,
}

impl<S, I> Cache<S, I> {
    /// Create new `Cache`.
    #[inline]
    pub fn new(store: S, issuer: I) -> Self {
        let skipper = MethodSkipper::new()
            .skip_all()
            .skip_get(false)
            .skip_post(false);
        Cache {
            store,
            issuer,
            skipper: Box::new(skipper),
        }
    }
}

#[async_trait]
impl<S, I> Handler for Cache<S, I>
where
    S: CacheStore<Key = I::Key>,
    I: CacheIssuer,
{
    async fn handle(
        &self,
        req: &mut Request,
        depot: &mut Depot,
        res: &mut Response,
        ctrl: &mut FlowCtrl,
    ) {
        if self.skipper.skipped(req, depot) {
            return;
        }
        let key = match self.issuer.issue(req, depot).await {
            Some(key) => key,
            None => {
                return;
            }
        };
        let cache = match self.store.load_entry(&key).await {
            Some(cache) => cache,
            None => {
                ctrl.call_next(req, depot, res).await;
                if !res.body.is_stream() && res.headers().get(CACHE_STATUS).is_some() {
                    let headers = res.headers().clone();

                    let body = TryInto::<CachedBody>::try_into(&res.body);
                    match body {
                        Ok(body) => {
                            let cached_data = CachedEntry::new(res.status_code, headers, body);
                            if let Err(e) = self.store.save_entry(key, cached_data).await {
                                error!("cache failed {e}");
                            }
                        }
                        Err(e) => error!("{e}"),
                    }
                    //let body: CachedBody = res.body().try_into().unwrap();
                }
                return;
            }
        };
        let CachedEntry {
            status,
            headers,
            body,
            ..
        } = cache;
        if let Some(status) = status {
            res.status_code(status);
        }
        *res.headers_mut() = headers;
        *res.body_mut() = body.into();
        ctrl.skip_rest();
    }
}
