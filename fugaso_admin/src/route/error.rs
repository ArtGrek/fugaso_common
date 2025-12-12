use essential_core::error::ServerError;
use essential_rand::error::RandError;
use reqwest::StatusCode;
use salvo::{Depot, Request, Response, Writer, __private::tracing::error, async_trait};

#[derive(Debug)]
pub enum LaunchError {
    Header(ErrData),
    Game(ErrData),
    Url(ErrData),
    Server(ErrData),
    Internal(ServerError),
    Rand(RandError),
    Parse(salvo::http::ParseError),
}

#[derive(Debug)]
pub struct ErrData {
    pub line: u32,
    pub file: &'static str,
    pub message: String,
}

#[macro_export]
macro_rules! err_on {
    ($expression: expr) => {
        ErrData {
            line: line!(),
            file: file!(),
            message: $expression.to_string(),
        }
    };
}

impl From<ServerError> for LaunchError {
    fn from(value: ServerError) -> Self {
        Self::Internal(value)
    }
}

impl From<salvo::http::ParseError> for LaunchError {
    fn from(value: salvo::http::ParseError) -> Self {
        Self::Parse(value)
    }
}

#[async_trait]
impl Writer for LaunchError {
    async fn write(self, _req: &mut Request, _depot: &mut Depot, res: &mut Response) {
        match self {
            LaunchError::Header(d) => {
                error!("{d:?}");
                res.status_code(StatusCode::BAD_REQUEST);
            }
            LaunchError::Server(d) | LaunchError::Url(d) => {
                error!("{d:?}");
                res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
            }
            LaunchError::Internal(e) => {
                error!("{e:?}");
                res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
            }
            LaunchError::Rand(e) => {
                error!("{e:?}");
                res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
            }
            LaunchError::Parse(e) => {
                error!("{e:?}");
                res.status_code(StatusCode::BAD_REQUEST);
            }
            LaunchError::Game(d) => {
                error!("{d:?}");
                res.status_code(StatusCode::NOT_FOUND);
            }
        }
    }
}
