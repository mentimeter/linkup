use linkup::{
    GetTunnelRequest, SessionResponse, TunnelData, TunneledSessionResponse, UpsertSessionRequest,
};
use reqwest::{StatusCode, header};
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

#[derive(Clone)]
pub struct WorkerClient {
    url: Url,
    inner: reqwest::Client,
}

impl WorkerClient {
    pub fn new(url: &Url, worker_token: &str) -> Self {
        let mut headers = header::HeaderMap::new();
        let mut auth_value = header::HeaderValue::from_str(&format!("Bearer {}", worker_token))
            .expect("token to contain only valid bytes");

        auth_value.set_sensitive(true);

        headers.insert(header::AUTHORIZATION, auth_value);
        headers.insert(
            "x-linkup-version",
            header::HeaderValue::from_static(CURRENT_VERSION),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("reqwest client to be valid and created");

        Self {
            url: url.clone(),
            inner: client,
        }
    }

    pub async fn tunneled_session(
        &self,
        params: &UpsertSessionRequest,
    ) -> Result<TunneledSessionResponse, Error> {
        self.post("/linkup/v2/sessions/tunneled", params).await
    }

    pub async fn preview_session(
        &self,
        params: &UpsertSessionRequest,
    ) -> Result<SessionResponse, Error> {
        self.post("/linkup/v2/sessions/preview", params).await
    }

    pub async fn get_tunnel(&self, session_name: &str) -> Result<TunnelData, Error> {
        let query = GetTunnelRequest {
            session_name: String::from(session_name),
        };

        let endpoint = self.url.join("/linkup/tunnel")?;
        let response = self.inner.get(endpoint).query(&query).send().await?;

        match response.status() {
            StatusCode::OK => {
                let content: TunnelData = response.json().await?;
                Ok(content)
            }
            _ => Err(Error::Response(
                response.status(),
                response.text().await.unwrap_or_else(|_| "".to_string()),
            )),
        }
    }

    // TODO(@augustoccesar)[2026-04-21]: This is the same on local_server. Can probably be combined
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

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
