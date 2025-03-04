use jane_eyre::eyre;
use rocket::{
    http::Status,
    response::{self, content::RawText, Responder},
    Request,
};
use tracing::warn;

// Most of this is lifted from <https://github.com/yuk1ty/rocket-errors/blob/b617f860d27d8f162e97e92311eeeba1abd38b95/src/eyre.rs>

/// A type alias with [`EyreReport`] to use `eyre::Result` in Rocket framework.
pub type Result<T, E = EyreReport> = std::result::Result<T, E>;

/// A wrapper of `eyre::Report` to be able to make use of `eyre` in Rocket framework.
/// [`rocket::response::Responder`] is implemented to this type.
#[derive(Debug)]
pub enum EyreReport {
    BadRequest(eyre::Report),
    InternalServerError(eyre::Report),
}

impl<E> From<E> for EyreReport
where
    E: Into<eyre::Report>,
{
    fn from(error: E) -> Self {
        // default to InternalServerError
        EyreReport::InternalServerError(error.into())
    }
}

impl<'r> Responder<'r, 'static> for EyreReport {
    fn respond_to(self, request: &Request<'_>) -> response::Result<'static> {
        let (status, error) = match self {
            Self::BadRequest(e) => (Status::BadRequest, e),
            Self::InternalServerError(e) => (Status::InternalServerError, e),
        };

        warn!("Error: {:?}", error);
        let mut res = RawText(format!("{:?}", error)).respond_to(request)?;
        res.set_status(status);
        Ok(res)
    }
}
