use std::time::Duration;

use linkup::{SessionResponse, TunneledSessionResponse, UpsertSessionRequest};
use reqwest::StatusCode;
use serde::{Serialize, de::DeserializeOwned};
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    UrlParse(#[from] url::ParseError),
    #[error("{0}")]
    Serde(#[from] serde_json::Error),
    #[error("request failed with status {0}: {1}")]
    Response(StatusCode, String),
}

pub struct LocalServerClient {
    url: Url,
    inner: reqwest::Client,
}

impl LocalServerClient {
    pub fn new(url: &Url) -> Self {
        Self {
            url: url.clone(),
            inner: reqwest::Client::new(),
        }
    }

    pub async fn health_check(&self) -> Result<bool, Error> {
        let endpoint = self.url.join("/linkup/check")?;

        let response = self
            .inner
            .get(endpoint)
            .timeout(Duration::from_secs(1))
            .send()
            .await?;

        Ok(matches!(response, res if res.status() == StatusCode::OK))
    }

    pub async fn preview_session(
        &self,
        params: &UpsertSessionRequest,
    ) -> Result<SessionResponse, Error> {
        self.post("/linkup/sessions/preview", params).await
    }

    pub async fn tunneled_session(
        &self,
        params: &UpsertSessionRequest,
    ) -> Result<TunneledSessionResponse, Error> {
        self.post("/linkup/sessions/tunneled", params).await
    }

    pub async fn local_only_session(
        &self,
        _params: &UpsertSessionRequest,
    ) -> Result<String, Error> {
        // TODO(@augustoccesar)[2026-04-23]: Implement
        todo!()
    }

    // TODO(@augustoccesar)[2026-04-21]: This is the same on worker. Can probably be combined
    async fn post<T: Serialize, R: DeserializeOwned>(
        &self,
        path: &str,
        params: &T,
    ) -> Result<R, Error> {
        let params = serde_json::to_string(params)?;
        let endpoint = self.url.join(path)?;
        let response = self
            .inner
            .post(endpoint)
            .header("Content-Type", "application/json")
            .body(params)
            .send()
            .await?;

        if response.status().is_success() {
            let content = response.json().await?;

            Ok(content)
        } else {
            Err(Error::Response(
                response.status(),
                response.text().await.unwrap_or_else(|_| "".to_string()),
            ))
        }
    }
}
