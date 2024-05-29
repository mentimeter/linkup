use std::{env, fs, path::Path};

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

use crate::file_system::{FileLike, FileSystem, RealFileSystem};
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

// Helper to create an HTTP client and prepare headers
fn prepare_client_and_headers() -> Result<(reqwest::blocking::Client, HeaderMap), CliError> {
    let bearer_token =
        env::var("LINKUP_CF_API_TOKEN").map_err(|err| CliError::BadConfig(err.to_string()))?;
    let client = reqwest::blocking::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", bearer_token))
            .map_err(|err| CliError::StatusErr(err.to_string()))?,
    );

    Ok((client, headers))
}

// Helper for sending requests and handling responses
fn send_request<T: for<'de> serde::Deserialize<'de>>(
    client: &reqwest::blocking::Client,
    url: &str,
    headers: HeaderMap,
    body: Option<String>,
    method: &str,
) -> Result<T, CliError> {
    let builder = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => return Err(CliError::StatusErr("Unsupported HTTP method".to_string())),
    };

    let builder = builder.headers(headers);
    let builder = if let Some(body) = body {
        builder.body(body)
    } else {
        builder
    };

    let response = builder.send().map_err(|err| {
        CliError::StatusErr(format!("Failed to send request, {}", err).to_string())
    })?;

    if response.status().is_success() {
        let response_body = response.text().map_err(|err| {
            CliError::StatusErr(format!("Could not read response body, {}", err).to_string())
        })?;
        serde_json::from_str(&response_body).map_err(|err| {
            CliError::StatusErr(
                format!(
                    "Could not parse JSON, {}. Response body: {}",
                    err, response_body
                )
                .to_string(),
            )
        })
    } else {
        Err(CliError::StatusErr(format!(
            "Failed to get a successful response: {}",
            response.status()
        )))
    }
}

#[cfg_attr(test, mockall::automock)]
pub trait PaidTunnelManager {
    fn get_tunnel_id(&self, tunnel_name: &str) -> Result<Option<String>, CliError>;
    fn create_tunnel(&self, tunnel_name: &str) -> Result<String, CliError>;
    fn create_dns_record(&self, tunnel_id: &str, tunnel_name: &str) -> Result<(), CliError>;
}

pub struct RealPaidTunnelManager;

impl PaidTunnelManager for RealPaidTunnelManager {
    fn get_tunnel_id(&self, tunnel_name: &str) -> Result<Option<String>, CliError> {
        let account_id = env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID")
            .map_err(|err| CliError::BadConfig(err.to_string()))?;
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
            account_id
        );
        let (client, headers) = prepare_client_and_headers()?;
        let query_url = format!("{}?name=tunnel-{}", url, tunnel_name);

        let parsed: GetTunnelApiResponse = send_request(&client, &query_url, headers, None, "GET")?;
        if parsed.result.is_empty() {
            Ok(None)
        } else {
            Ok(Some(parsed.result[0].id.clone()))
        }
    }

    fn create_tunnel(&self, tunnel_name: &str) -> Result<String, CliError> {
        let tunnel_secret = generate_tunnel_secret();
        let account_id = env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID")
            .map_err(|err| CliError::BadConfig(err.to_string()))?;
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
            account_id,
        );
        let (client, headers) = prepare_client_and_headers()?;
        let body = serde_json::to_string(&CreateTunnelRequest {
            name: format!("tunnel-{}", tunnel_name),
            tunnel_secret: tunnel_secret.clone(),
        })
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

        let parsed: CreateTunnelResponse =
            send_request(&client, &url, headers, Some(body), "POST")?;
        save_tunnel_credentials(&RealFileSystem, &parsed.result.id, &tunnel_secret)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        create_config_yml(&RealFileSystem, &parsed.result.id)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;

        Ok(parsed.result.id)
    }

    fn create_dns_record(&self, tunnel_id: &str, tunnel_name: &str) -> Result<(), CliError> {
        let zone_id = env::var("LINKUP_CLOUDFLARE_ZONE_ID")
            .map_err(|err| CliError::BadConfig(err.to_string()))?;
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
            zone_id
        );
        let (client, headers) = prepare_client_and_headers()?;
        let body = serde_json::to_string(&DNSRecord {
            name: format!("tunnel-{}", tunnel_name),
            content: format!("{}.cfargotunnel.com", tunnel_id),
            r#type: "CNAME".to_string(),
            proxied: true,
        })
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

        println!("{}", body);

        let _parsed: CreateDNSRecordResponse =
            send_request(&client, &url, headers, Some(body), "POST")?;
        Ok(())
    }
}

fn generate_tunnel_secret() -> String {
    let mut rng = rand::thread_rng();
    let random_bytes: [u8; 32] = rng.gen();
    BASE64_STANDARD.encode(random_bytes)
}

fn save_tunnel_credentials(
    filesys: &dyn FileSystem,
    tunnel_id: &str,
    tunnel_secret: &str,
) -> Result<(), CliError> {
    let account_id = filesys
        .get_env("LINKUP_CLOUDFLARE_ACCOUNT_ID")
        .map_err(|err| CliError::BadConfig(err.to_string()))?;
    let data = serde_json::json!({
        "AccountTag": account_id,
        "TunnelID": tunnel_id,
        "TunnelSecret": tunnel_secret,
    });

    // Determine the directory path
    let home_dir = filesys
        .get_env("HOME")
        .map_err(|err| CliError::BadConfig(err.to_string()))?;
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    // Create the directory if it does not exist
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path).map_err(|err| CliError::StatusErr(err.to_string()))?;
    }

    // Define the file path
    let file_path = dir_path.join(format!("{}.json", tunnel_id));

    // Create and write to the file
    let mut file: Box<dyn FileLike> = filesys
        .create_file(file_path)
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    let data_string = data.to_string();
    filesys
        .write_file(&mut file, &data_string)
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

    Ok(())
}

fn create_config_yml(filesys: &dyn FileSystem, tunnel_id: &str) -> Result<(), CliError> {
    // Determine the directory path
    let home_dir = filesys
        .get_env("HOME")
        .map_err(|err| CliError::BadConfig(err.to_string()))?;
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    // Create the directory if it does not exist
    if !filesys.file_exists(dir_path.as_path()) {
        println!("Creating directory: {:?}", dir_path);
        filesys
            .create_dir_all(&dir_path)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
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

    let mut file: Box<dyn FileLike> = filesys
        .create_file(dir_path.join("config.yml"))
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    filesys
        .write_file(&mut file, &serialized)
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::file_system::MockFileSystem;

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
        env::set_var("LINKUP_CF_API_TOKEN", "TOKEN");
        let result = prepare_client_and_headers();
        env::remove_var("LINKUP_CF_API_TOKEN");
        assert!(result.is_ok());
        let (_client, headers) = result.unwrap();
        assert!(headers.contains_key("Authorization"));
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer TOKEN");
    }

    #[test]
    fn create_config_yml_when_no_config_dir() {
        let content = "url: http://localhost:9066\ntunnel: TUNNEL_ID\ncredentials-file: /tmp/home/.cloudflared/TUNNEL_ID.json\n";

        let mut mock_fs = MockFileSystem::new();
        // If .cloudflared directory does not exist:
        mock_fs
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));
        mock_fs
            .expect_file_exists()
            .withf(|path| path.ends_with(".cloudflared"))
            .returning(|_| false);
        // Create .cloudflared directory:
        mock_fs
            .expect_create_dir_all()
            .with(predicate::eq(Path::new("/tmp/home/.cloudflared")))
            .returning(|_| Ok(()));
        // Create config.yml file:
        mock_fs
            .expect_create_file()
            .withf(|path| path.ends_with("config.yml"))
            .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));
        // Write to config.yml file:
        mock_fs
            .expect_write_file()
            .with(predicate::always(), predicate::eq(content))
            .returning(|_, _| Ok(()));

        let result = create_config_yml(&mock_fs, "TUNNEL_ID");
        assert!(result.is_ok());
    }

    #[test]
    fn create_config_yml_config_dir_exists() {
        let content = "url: http://localhost:9066\ntunnel: TUNNEL_ID\ncredentials-file: /tmp/home/.cloudflared/TUNNEL_ID.json\n";

        let mut mock_fs = MockFileSystem::new();
        mock_fs
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));
        // If .cloudflared directory exists:
        mock_fs
            .expect_file_exists()
            .withf(|path| path.ends_with(".cloudflared"))
            .returning(|_| true);
        // Don't create .cloudflared directory:
        mock_fs.expect_create_dir_all().never();
        // Create/truncate config.yml file:
        mock_fs
            .expect_create_file()
            .withf(|path| path.ends_with("config.yml"))
            .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));
        // Write to config.yml file:
        mock_fs
            .expect_write_file()
            .with(predicate::always(), predicate::eq(content))
            .returning(|_, _| Ok(()));

        let result = create_config_yml(&mock_fs, "TUNNEL_ID");
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_tunnel_credentials() {
        let content = "{\"AccountTag\":\"ACCOUNT_ID\",\"TunnelID\":\"TUNNEL_ID\",\"TunnelSecret\":\"AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg=\"}";

        let mut mock_fs = MockFileSystem::new();
        mock_fs
            .expect_get_env()
            .with(predicate::eq("HOME"))
            .returning(|_| Ok("/tmp/home".to_string()));

        mock_fs
            .expect_get_env()
            .with(predicate::eq("LINKUP_CLOUDFLARE_ACCOUNT_ID"))
            .returning(|_| Ok("ACCOUNT_ID".to_string()));

        mock_fs
            .expect_create_file()
            .withf(|path| path.ends_with("TUNNEL_ID.json"))
            .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));
        mock_fs
            .expect_write_file()
            .with(predicate::always(), predicate::eq(content))
            .returning(|_, _| Ok(()));

        let result = save_tunnel_credentials(
            &mock_fs,
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
