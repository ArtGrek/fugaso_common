use salvo::http::header::CONTENT_TYPE;
use salvo::http::HeaderValue;
use salvo::prelude::StatusCode;
use salvo::{handler, Depot, FlowCtrl, Request, Response};

#[handler]
pub async fn options_handle(
    _req: &mut Request,
    _depot: &mut Depot,
    res: &mut Response,
    _ctrl: &mut FlowCtrl,
) {
    res.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    res.status_code(StatusCode::OK);
}
