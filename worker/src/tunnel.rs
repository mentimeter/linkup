// TODO: Replace String errors for proper error Enum

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct GetTunnelApiResponse {
    result: Vec<TunnelResultItem>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TunnelResultItem {
    id: String,
    name: String,
    deleted_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct TokenApiResponse {
    result: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct CreateTunnelRequest {
    name: String,
    tunnel_secret: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct CreateDNSRecordResponse {
    result: DNSRecord,
}

#[derive(Serialize, Deserialize, Debug)]
struct DNSRecord {
    content: String,
    name: String,
    r#type: String,
    proxied: bool,
}
#[derive(Serialize, Deserialize, Debug)]
struct CreateTunnelResponse {
    result: TunnelResultItem,
}

#[derive(Serialize, Deserialize)]
struct Config {
    url: String,
    tunnel: String,
    #[serde(rename = "credentials-file")]
    credentials_file: String,
}

pub async fn _get_tunnel_id(
    api_token: &str,
    account_id: &str,
    tunnel_name: &str,
) -> Result<Option<String>, String> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
        account_id
    );
    let (client, headers) = prepare_client_and_headers(api_token)?;
    let query_url = format!("{}?name={}", url, tunnel_name);

    let parsed: GetTunnelApiResponse =
        send_request(&client, &query_url, headers, None, "GET").await?;
    if parsed.result.is_empty() {
        Ok(None)
    } else {
        // Check if there exists a tunnel with this name that hasn't been deleted
        match parsed
            .result
            .iter()
            .find(|tunnel| tunnel.deleted_at.is_none())
        {
            Some(tunnel) => Ok(Some(tunnel.id.clone())),
            None => Ok(None),
        }
    }
}

pub async fn create_tunnel(
    api_token: &str,
    account_id: &str,
    tunnel_name: &str,
    // TODO: Make this tuple into a proper type
) -> Result<(String, String), String> {
    let tunnel_secret = generate_tunnel_secret();
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
        account_id,
    );
    let (client, headers) = prepare_client_and_headers(api_token)?;
    let body = serde_json::to_string(&CreateTunnelRequest {
        name: tunnel_name.to_string(),
        tunnel_secret: tunnel_secret.clone(),
    })
    .expect("body to be valid");

    let parsed: CreateTunnelResponse =
        send_request(&client, &url, headers, Some(body), "POST").await?;

    Ok((parsed.result.id, tunnel_name.to_string()))
}

pub async fn create_dns_record(
    api_token: &str,
    zone_id: &str,
    tunnel_id: &str,
    tunnel_name: &str,
) -> Result<(), String> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        zone_id
    );
    let (client, headers) = prepare_client_and_headers(api_token)?;
    let body = serde_json::to_string(&DNSRecord {
        name: tunnel_name.to_string(),
        content: format!("{}.cfargotunnel.com", tunnel_id),
        r#type: "CNAME".to_string(),
        proxied: true,
    })
    .expect("body to be valid");

    let _parsed: CreateDNSRecordResponse =
        send_request(&client, &url, headers, Some(body), "POST").await?;
    Ok(())
}

pub async fn get_zone_domain(api_token: &str, zone_id: &str) -> String {
    #[derive(Deserialize)]
    struct ZoneResponseResult {
        name: String,
    }

    #[derive(Deserialize)]
    struct ZoneResponse {
        result: ZoneResponseResult,
    }

    let url = format!("https://api.cloudflare.com/client/v4/zones/{}", &zone_id);
    let (client, headers) =
        prepare_client_and_headers(api_token).expect("client to be proper built");

    let zone_response: ZoneResponse = send_request(&client, &url, headers, None, "GET")
        .await
        .unwrap();

    zone_response.result.name
}

// Helper to create an HTTP client and prepare headers
fn prepare_client_and_headers(api_token: &str) -> Result<(reqwest::Client, HeaderMap), String> {
    // this should be a string, not a result
    // let bearer_token = sys.get_env("LINKUP_CF_API_TOKEN")?;
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_token)).expect("api_token should be valid"),
    );

    Ok((client, headers))
}

// Helper for sending requests and handling responses
async fn send_request<T: for<'de> serde::Deserialize<'de>>(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    body: Option<String>,
    method: &str,
) -> Result<T, String> {
    let builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => return Err("Invalid HTTP method".into()),
    };

    let builder = builder.headers(headers);
    let builder = if let Some(body) = body {
        builder.body(body)
    } else {
        builder
    };

    let response = builder.send().await.unwrap();

    if response.status().is_success() {
        let response_body = response.text().await.unwrap();
        serde_json::from_str(&response_body).unwrap()
    } else {
        Err("Wot".into())
    }
}

fn generate_tunnel_secret() -> String {
    // TODO: Use rand and getrandom (with 'js' feature)
    // let mut rng = rand::thread_rng();
    // let random_bytes: [u8; 32] = rng.gen();
    // BASE64_STANDARD.encode(random_bytes)

    "suppasecret".into()
}
