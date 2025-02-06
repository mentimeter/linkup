use auth::CloudflareApiAuth;

mod auth;
// use crate::auth::CloudflareApiAuth;
// use crate::client::ApiClient;
// use crate::domain::dns::DnsApi;
// use crate::domain::kv::KvApi;
// use crate::domain::tokens::TokensApi;
// use crate::domain::workers::WorkersApi;
// use std::sync::Arc;

pub struct CloudflareApi {
    // pub workers: Arc<dyn WorkersApi + Send + Sync>,
    // pub kv: Arc<dyn KvApi + Send + Sync>,
    // pub dns: Arc<dyn DnsApi + Send + Sync>,
    // pub tokens: Arc<dyn TokensApi + Send + Sync>,
    // // … possibly other domains …
}

impl CloudflareApi {
    pub fn new(account_id: String, api_auth: Box<dyn CloudflareApiAuth>) -> Self {
        // Create a common client instance.
        // let client = ApiClient::new();

        // // Create real domain implementations (passing shared client, auth, etc.)
        // let workers = crate::domain::workers::RealWorkersApi {
        //     account_id: account_id.clone(), /*, client: client.clone(), etc. */
        // };
        // let kv = crate::domain::kv::RealKvApi {
        //     account_id: account_id.clone(), /*, ... */
        // };
        // let dns = crate::domain::dns::RealDnsApi {
        //     account_id: account_id.clone(), /*, ... */
        // };
        // let tokens = crate::domain::tokens::RealTokensApi {
        //     account_id, /*, ... */
        // };

        Self {
            // workers: Arc::new(workers),
            // kv: Arc::new(kv),
            // dns: Arc::new(dns),
            // tokens: Arc::new(tokens),
        }
    }
}
