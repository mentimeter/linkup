use linkup::{CreatePreviewRequest, UpdateSessionRequest};
use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::local_config::YamlLocalConfig;

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
    #[error("your session is in an inconsistent state. Stop your session before trying again.")]
    InconsistentState,
}

pub struct WorkerClient {
    url: Url,
    inner: reqwest::Client,
}

// TODO: This is a copy of the TunnelData from worker. We can/should probably have a shared one.
#[derive(Serialize, Deserialize)]
pub struct TunnelData {
    pub account_id: String,
    pub name: String,
    pub url: String,
    pub id: String,
    pub secret: String,
    pub last_started: u64,
}

// TODO: This is a copy of the GetTunnelParams from worker. We can/should probably have a shared one.
#[derive(Serialize)]
struct GetTunnelParams {
    session_name: String,
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
            header::HeaderValue::from_static(crate::CURRENT_VERSION),
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

    pub async fn preview(&self, params: &CreatePreviewRequest) -> Result<String, Error> {
        self.post("/linkup/preview-session", params).await
    }

    pub async fn linkup(&self, params: &UpdateSessionRequest) -> Result<String, Error> {
        self.post("/linkup/local-session", params).await
    }

    pub async fn get_tunnel(&self, session_name: &str) -> Result<TunnelData, Error> {
        let query = GetTunnelParams {
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

    async fn post<T: Serialize>(&self, path: &str, params: &T) -> Result<String, Error> {
        let params = serde_json::to_string(params)?;
        let endpoint = self.url.join(path)?;
        let response = self
            .inner
            .post(endpoint)
            .header("Content-Type", "application/json")
            .body(params)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {
                let content = response.text().await?;
                Ok(content)
            }
            _ => Err(Error::Response(
                response.status(),
                response.text().await.unwrap_or_else(|_| "".to_string()),
            )),
        }
    }
}

impl From<&YamlLocalConfig> for WorkerClient {
    fn from(config: &YamlLocalConfig) -> Self {
        Self::new(&config.linkup.worker_url, &config.linkup.worker_token)
    }
}
