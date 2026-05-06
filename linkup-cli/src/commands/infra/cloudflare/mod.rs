pub(super) mod api;
pub(super) mod auth;
pub(super) mod resources;

#[derive(thiserror::Error, Debug)]
pub enum DeployError {
    #[error("Cloudflare API error: {0}")]
    CloudflareApiError(#[from] reqwest::Error),
    #[error("Cloudflare Client error: {0}")]
    CloudflareClientError(#[from] cloudflare::framework::response::ApiFailure),
    #[error("Unexpected Cloudflare API response: {0}")]
    UnexpectedResponse(String),
    #[error("Other failure")]
    OtherError,
}
