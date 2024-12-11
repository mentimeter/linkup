use linkup::{CreatePreviewRequest, UpdateSessionRequest};
use reqwest::StatusCode;
use serde::Serialize;
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
}

pub struct WorkerClient {
    url: Url,
    inner: reqwest::Client,
}

impl WorkerClient {
    pub fn new(url: &Url) -> Self {
        Self {
            url: url.clone(),
            inner: reqwest::Client::new(),
        }
    }

    pub async fn preview(&self, params: &CreatePreviewRequest) -> Result<String, Error> {
        self.post("/preview", params).await
    }

    pub async fn linkup(&self, params: &UpdateSessionRequest) -> Result<String, Error> {
        self.post("/linkup", params).await
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
        Self::new(&config.linkup.remote)
    }
}
