use crate::framework::{
    auth,
    auth::{AuthClient, Credentials},
    endpoint::Endpoint,
    response::{ApiErrors, ApiFailure, ApiSuccess},
    response::{ApiResponse, ApiResult},
    Environment, HttpApiClientConfig,
};

/// A Cloudflare API client that makes requests asynchronously.
pub struct Client {
    environment: Environment,
    credentials: auth::Credentials,
    http_client: reqwest::Client,
}

impl AuthClient for reqwest::RequestBuilder {
    fn auth(mut self, credentials: &Credentials) -> Self {
        for (k, v) in credentials.headers() {
            self = self.header(k, v);
        }
        self
    }
}

impl Client {
    pub fn new(
        credentials: auth::Credentials,
        config: HttpApiClientConfig,
        environment: Environment,
    ) -> Result<Client, crate::framework::Error> {
        #[allow(unused_mut)]
        let mut builder = reqwest::Client::builder().default_headers(config.default_headers);

        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::net::SocketAddr;

            // There is no resolve method in wasm.
            if let Some(address) = config.resolve_ip {
                let url = url::Url::from(&environment);
                builder = builder.resolve(
                    url.host_str()
                        .expect("Environment url should have a hostname"),
                    SocketAddr::new(address, 443),
                );
            }

            // There are no timeouts in wasm. The property is documented as no-op in wasm32.
            builder = builder.timeout(config.http_timeout);
        }

        let http_client = builder.build()?;

        Ok(Client {
            environment,
            credentials,
            http_client,
        })
    }

    /// Issue an API request of the given type.
    pub async fn request<ResultType>(
        &self,
        endpoint: &(dyn Endpoint<ResultType> + Send + Sync),
    ) -> ApiResponse<ResultType>
    where
        ResultType: ApiResult,
    {
        // Build the request
        let mut request = self
            .http_client
            .request(endpoint.method(), endpoint.url(&self.environment));

        if let Some(body) = endpoint.body() {
            request = request.body(body);
            request = request.header(
                reqwest::header::CONTENT_TYPE,
                endpoint.content_type().as_ref(),
            );
        }

        request = request.auth(&self.credentials);
        let response = request.send().await?;
        map_api_response(response).await
    }
}

// If the response is 2XX and parses, return Success.
// If the response is 2XX and doesn't parse, return Invalid.
// If the response isn't 2XX, return Failure, with API errors if they were included.
async fn map_api_response<ResultType: ApiResult>(
    resp: reqwest::Response,
) -> ApiResponse<ResultType> {
    let status = resp.status();
    if status.is_success() {
        let parsed: Result<ApiSuccess<ResultType>, reqwest::Error> = resp.json().await;
        match parsed {
            Ok(api_resp) => Ok(api_resp),
            Err(e) => Err(ApiFailure::Invalid(e)),
        }
    } else {
        let parsed: Result<ApiErrors, reqwest::Error> = resp.json().await;
        let errors = parsed.unwrap_or_default();
        Err(ApiFailure::Error(status, errors))
    }
}
