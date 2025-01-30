use std::{env, fs, path::Path};

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

use crate::system::{FileLike, RealSystem, System};
use crate::{CliError, LINKUP_LOCALSERVER_PORT};
use serde::{Deserialize, Serialize};

use base64::prelude::*;
use rand::Rng;

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

pub async fn get_tunnel_id(tunnel_name: &str) -> Result<Option<String>, CliError> {
    let account_id = env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID")
        .map_err(|_| CliError::GetEnvVar("LINKUP_CLOUDFLARE_ACCOUNT_ID".to_string()))?;
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
        account_id
    );
    let (client, headers) = prepare_client_and_headers(&RealSystem)?;
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

pub async fn create_tunnel(tunnel_name: &str) -> Result<String, CliError> {
    let tunnel_secret = generate_tunnel_secret();
    let account_id = env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID")
        .map_err(|_| CliError::GetEnvVar("LINKUP_CLOUDFLARE_ACCOUNT_ID".to_string()))?;
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
        account_id,
    );
    let (client, headers) = prepare_client_and_headers(&RealSystem)?;
    let body = serde_json::to_string(&CreateTunnelRequest {
        name: tunnel_name.to_string(),
        tunnel_secret: tunnel_secret.clone(),
    })
    .map_err(|err| CliError::ParseErr("from JSON to String".to_string(), err.to_string()))?;

    let parsed: CreateTunnelResponse =
        send_request(&client, &url, headers, Some(body), "POST").await?;
    save_tunnel_credentials(&RealSystem, &parsed.result.id, &tunnel_secret).map_err(|err| {
        CliError::FileErr(
            "Failed to save tunnel credentials".to_string(),
            err.to_string(),
        )
    })?;
    create_config_yml(&RealSystem, &parsed.result.id).map_err(|err| {
        CliError::FileErr("Failed to create config YML".to_string(), err.to_string())
    })?;

    Ok(parsed.result.id)
}

pub async fn create_dns_record(
    tunnel_id: &str,
    tunnel_name: &str,
    zone_id: &str,
) -> Result<(), CliError> {
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        zone_id
    );
    let (client, headers) = prepare_client_and_headers(&RealSystem)?;
    let body = serde_json::to_string(&DNSRecord {
        name: tunnel_name.to_string(),
        content: format!("{}.cfargotunnel.com", tunnel_id),
        r#type: "CNAME".to_string(),
        proxied: true,
    })
    .map_err(|err| CliError::ParseErr("from JSON to String".to_string(), err.to_string()))?;

    let _parsed: CreateDNSRecordResponse =
        send_request(&client, &url, headers, Some(body), "POST").await?;
    Ok(())
}

pub async fn get_tunnel_zone_id() -> Result<Option<(String, String)>, CliError> {
    let url = "https://api.cloudflare.com/client/v4/zones";
    let (client, headers) = match prepare_client_and_headers(&RealSystem) {
        Ok(result) => result,
        Err(_) => return Ok(None),
    };

    let response_body: serde_json::Value = send_request(&client, url, headers, None, "GET").await?;

    if let Some(result_array) = response_body.get("result").and_then(|r| r.as_array()) {
        for zone in result_array {
            if let (Some(id), Some(name), Some(permissions)) = (
                zone.get("id").and_then(|id| id.as_str()),
                zone.get("name").and_then(|id| id.as_str()),
                zone.get("permissions").and_then(|p| p.as_array()),
            ) {
                let has_dns_edit = permissions
                    .iter()
                    .any(|p| p.as_str() == Some("#dns_records:edit"));
                let has_zone_read = permissions.iter().any(|p| p.as_str() == Some("#zone:read"));

                if has_dns_edit && has_zone_read {
                    return Ok(Some((id.to_string(), name.to_string())));
                }
            }
        }
    }

    Ok(None)
}

// Helper to create an HTTP client and prepare headers
fn prepare_client_and_headers(sys: &dyn System) -> Result<(reqwest::Client, HeaderMap), CliError> {
    // this should be a string, not a result
    let bearer_token = sys.get_env("LINKUP_CLOUDFLARE_API_TOKEN")?;
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", bearer_token))
            .map_err(|_| CliError::GetEnvVar("LINKUP_CLOUDFLARE_API_TOKEN".to_string()))?,
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
) -> Result<T, CliError> {
    let builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => {
            return Err(CliError::HttpErr(format!(
                "Unsupported HTTP method: {}",
                method
            )))
        }
    };

    let builder = builder.headers(headers);
    let builder = if let Some(body) = body {
        builder.body(body)
    } else {
        builder
    };

    let response = builder
        .send()
        .await
        .map_err(|err| CliError::HttpErr(format!("Failed to send request, {}", err).to_string()))?;

    if response.status().is_success() {
        let response_body = response.text().await.map_err(|err| {
            CliError::HttpErr(format!("Could not read response body, {}", err).to_string())
        })?;
        serde_json::from_str(&response_body)
            .map_err(|err| CliError::ParseErr("from str to JSON".to_string(), err.to_string()))
    } else {
        Err(CliError::HttpErr(format!(
            "Failed to get a successful response: {}",
            response.status()
        )))
    }
}

fn generate_tunnel_secret() -> String {
    let mut rng = rand::thread_rng();
    let random_bytes: [u8; 32] = rng.gen();
    BASE64_STANDARD.encode(random_bytes)
}

fn save_tunnel_credentials(
    sys: &dyn System,
    tunnel_id: &str,
    tunnel_secret: &str,
) -> Result<(), CliError> {
    let account_id = sys
        .get_env("LINKUP_CLOUDFLARE_ACCOUNT_ID")
        .map_err(|_| CliError::GetEnvVar("LINKUP_CLOUDFLARE_ACCOUNT_ID".to_string()))?;
    let data = serde_json::json!({
        "AccountTag": account_id,
        "TunnelID": tunnel_id,
        "TunnelSecret": tunnel_secret,
    });

    // Determine the directory path
    let home_dir = sys
        .get_env("HOME")
        .map_err(|_| CliError::GetEnvVar("HOME".to_string()))?;
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    // Create the directory if it does not exist
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path).map_err(|err| {
            CliError::FileErr("Could not create directory".to_string(), err.to_string())
        })?
    }

    // Define the file path
    let file_path = dir_path.join(format!("{}.json", tunnel_id));

    // Create and write to the file
    let mut file: Box<dyn FileLike> = sys
        .create_file(file_path)
        .map_err(|err| CliError::FileErr("Could not create file".to_string(), err.to_string()))?;
    let data_string = data.to_string();
    sys.write_file(&mut file, &data_string)
        .map_err(|err| CliError::FileErr("Could not write to file".to_string(), err.to_string()))?;

    Ok(())
}

fn create_config_yml(sys: &dyn System, tunnel_id: &str) -> Result<(), CliError> {
    // Determine the directory path
    let home_dir = sys
        .get_env("HOME")
        .map_err(|_| CliError::GetEnvVar("HOME".to_string()))?;
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    // Create the directory if it does not exist
    if !sys.file_exists(dir_path.as_path()) {
        log::info!("Creating directory: {:?}", dir_path);
        sys.create_dir_all(&dir_path).map_err(|err| {
            CliError::FileErr("Could not create directory".to_string(), err.to_string())
        })?;
    }

    // Define the file path
    let file_path = dir_path.join(format!("{}.json", tunnel_id));
    let file_path_str = file_path.to_string_lossy().to_string();

    let config = Config {
        url: format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT),
        tunnel: tunnel_id.to_string(),
        credentials_file: file_path_str,
    };

    let serialized = serde_yaml::to_string(&config).expect("Failed to serialize config");

    let mut file: Box<dyn FileLike> = sys
        .create_file(dir_path.join("config.yml"))
        .map_err(|err| CliError::FileErr("Could not create file".to_string(), err.to_string()))?;
    sys.write_file(&mut file, &serialized)
        .map_err(|err| CliError::FileErr("Could not write to file".to_string(), err.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::system::MockSystem;

    use mockall::predicate;
    use std::io::{Read, Result as IoResult, Write};
    struct MockFile {
        content: Vec<u8>,
    }

    impl MockFile {
        fn new() -> MockFile {
            MockFile {
                content: Vec::new(),
            }
        }
    }

    impl FileLike for MockFile {}

    impl Read for MockFile {
        fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
            let amount = std::cmp::min(buf.len(), self.content.len());
            buf[..amount].copy_from_slice(&self.content[..amount]);
            Ok(amount)
        }
    }

    impl Write for MockFile {
        fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
            self.content.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }

    #[test]
    fn test_prepare_client_and_headers() {
        let mut mock_sys = MockSystem::new();

        mock_sys
            .expect_get_env()
            .with(predicate::eq("LINKUP_CLOUDFLARE_API_TOKEN"))
            .returning(|_| Ok("TOKEN".to_string()));

        let result = prepare_client_and_headers(&mock_sys);
        assert!(result.is_ok());
        let (_client, headers) = result.unwrap();
        assert!(headers.contains_key("Authorization"));
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer TOKEN");
    }

    #[test]
    #[should_panic(expected = "LINKUP_CLOUDFLARE_API_TOKEN is not set")]
    fn test_prepare_client_and_headers_token_env_var_not_set() {
        let mut mock_sys = MockSystem::new();

        mock_sys
            .expect_get_env()
            .with(predicate::eq("LINKUP_CLOUDFLARE_API_TOKEN"))
            .returning(|_| panic!("LINKUP_CLOUDFLARE_API_TOKEN is not set"));

        let result = prepare_client_and_headers(&mock_sys);
        assert!(result.is_err());
    }

    #[test]
    fn create_config_yml_when_no_config_dir() {
        let mut mock_sys = MockSystem::new();
        let content = "url: http://localhost:9066\ntunnel: TUNNEL_ID\ncredentials-file: /tmp/home/.cloudflared/TUNNEL_ID.json\n";

        mock_sys
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        // Check if .cloudflared directory exists -> false
        mock_sys
            .expect_file_exists()
            .withf(|path| path.ends_with(".cloudflared"))
            .returning(|_| false);

        // Create .cloudflared directory
        mock_sys
            .expect_create_dir_all()
            .with(predicate::eq(Path::new("/tmp/home/.cloudflared")))
            .returning(|_| Ok(()));

        // Create/truncate config.yml file
        mock_sys
            .expect_create_file()
            .withf(|path| path.ends_with("config.yml"))
            .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));

        // Write to config.yml file
        mock_sys
            .expect_write_file()
            .with(predicate::always(), predicate::eq(content))
            .returning(|_, _| Ok(()));

        let result = create_config_yml(&mock_sys, "TUNNEL_ID");
        assert!(result.is_ok());
    }

    #[test]
    fn create_config_yml_config_dir_exists() {
        let mut mock_sys = MockSystem::new();
        let content = "url: http://localhost:9066\ntunnel: TUNNEL_ID\ncredentials-file: /tmp/home/.cloudflared/TUNNEL_ID.json\n";

        mock_sys
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        // Check if .cloudflared directory exists -> true
        mock_sys
            .expect_file_exists()
            .withf(|path| path.ends_with(".cloudflared"))
            .returning(|_| true);

        // Don't create .cloudflared directory
        mock_sys.expect_create_dir_all().never();

        // Create/truncate config.yml file
        mock_sys
            .expect_create_file()
            .withf(|path| path.ends_with("config.yml"))
            .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));

        // Write to config.yml file
        mock_sys
            .expect_write_file()
            .with(predicate::always(), predicate::eq(content))
            .returning(|_, _| Ok(()));

        let result = create_config_yml(&mock_sys, "TUNNEL_ID");
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_tunnel_credentials() {
        let mut mock_sys = MockSystem::new();
        let content = "{\"AccountTag\":\"ACCOUNT_ID\",\"TunnelID\":\"TUNNEL_ID\",\"TunnelSecret\":\"AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg=\"}";

        mock_sys
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        mock_sys
            .expect_get_env()
            .with(predicate::eq("LINKUP_CLOUDFLARE_ACCOUNT_ID"))
            .returning(|_| Ok("ACCOUNT_ID".to_string()));

        // Create/truncate TUNNEL_ID.json file
        mock_sys
            .expect_create_file()
            .withf(|path| path.ends_with("TUNNEL_ID.json"))
            .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));

        // Write to TUNNEL_ID.json file
        mock_sys
            .expect_write_file()
            .with(predicate::always(), predicate::eq(content))
            .returning(|_, _| Ok(()));

        let result = save_tunnel_credentials(
            &mock_sys,
            "TUNNEL_ID",
            "AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg=",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_tunnel_secret() {
        let secret = generate_tunnel_secret();
        assert_eq!(secret.len(), 44);
    }
}
