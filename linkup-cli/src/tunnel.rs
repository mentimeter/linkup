use std::{
    env, fs::{self, File},
    io::{Read, Write, Result as IoResult},
    path::{Path, PathBuf},
};

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

use crate::CliError;
use serde::{Deserialize, Serialize};
use serde_yaml;
use mockall::{automock, predicate::*};

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
struct CreateDNSRecordRequest {
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

    let response = builder
        .send()
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

    if response.status().is_success() {
        let response_body = response
            .text()
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        serde_json::from_str(&response_body).map_err(|err| CliError::StatusErr(err.to_string()))
    } else {
        Err(CliError::StatusErr(format!(
            "Failed to get a successful response: {}",
            response.status()
        )))
    }
}

pub fn get_tunnel_id(tunnel_name: &str) -> Result<Option<String>, CliError> {
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

pub fn create_tunnel(tunnel_name: &str) -> Result<String, CliError> {
    let tunnel_secret = "AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg=".to_string(); // This is a hardcoded secret, it should be generated
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

    let parsed: CreateTunnelResponse = send_request(&client, &url, headers, Some(body), "POST")?;
    save_tunnel_credentials(&parsed.result.id, &tunnel_secret)
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    create_config_yml(&RealFileCreator,&RealFileWriter, &parsed.result.id).map_err(|err| CliError::StatusErr(err.to_string()))?;

    Ok(parsed.result.id)
}

fn save_tunnel_credentials(tunnel_id: &str, tunnel_secret: &str) -> Result<(), CliError> {
    let account_id = env::var("LINKUP_CLOUDFLARE_ACCOUNT_ID")
        .map_err(|err| CliError::BadConfig(err.to_string()))?;
    let data = serde_json::json!({
        "AccountTag": account_id,
        "TunnelSecret": tunnel_secret,
        "TunnelID": tunnel_id,
    });

    // Determine the directory path
    let home_dir = env::var("HOME").map_err(|err| CliError::BadConfig(err.to_string()))?;
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    // Create the directory if it does not exist
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path).map_err(|err| CliError::StatusErr(err.to_string()))?;
    }

    // Define the file path
    let file_path = dir_path.join(format!("{}.json", tunnel_id));

    // Create and write to the file
    let mut file = File::create(file_path).map_err(|err| CliError::StatusErr(err.to_string()))?;
    write!(file, "{}", data.to_string()).map_err(|err| CliError::StatusErr(err.to_string()))?;

    Ok(())
}

fn create_config_yml(creator: &dyn FileCreator, writer: &dyn FileWriter, tunnel_id: &str) -> Result<(), CliError> {
    // Determine the directory path
    let home_dir = env::var("HOME").map_err(|err| CliError::BadConfig(err.to_string()))?;
    let dir_path = Path::new(&home_dir).join(".cloudflared");

    // Create the directory if it does not exist
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path).map_err(|err| CliError::StatusErr(err.to_string()))?;
    }

    // Define the file path
    let file_path = dir_path.join(format!("{}.json", tunnel_id));
    let file_path_str = file_path.to_string_lossy().to_string();

    let config = Config {
        url: "http://localhost:8000".to_string(),
        tunnel: tunnel_id.to_string(),
        credentials_file: file_path_str,
    };

    let serialized = serde_yaml::to_string(&config).expect("Failed to serialize config");

    let mut file: Box<dyn FileLike> = creator.create_file(dir_path.join("config.yml"))
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    writer.write_file(&mut file, serialized.as_bytes())
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    Ok(())
}

pub fn create_dns_record(tunnel_id: &str, tunnel_name: &str) -> Result<(), CliError> {
    //let zone_id = env::var("LINKUP_CLOUDFLARE_ZONE_ID").map_err(|err| CliError::BadConfig(err.to_string()) )?;
    let zone_id = "ZONE_ID";
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/dns_records",
        zone_id
    );
    let (client, headers) = prepare_client_and_headers()?;
    let body = serde_json::to_string(&CreateDNSRecordRequest {
        name: format!("tunnel-{}", tunnel_name),
        content: format!("{}.cfargotunnel.com", tunnel_id),
        r#type: "CNAME".to_string(),
        proxied: true,
    })
    .map_err(|err| CliError::StatusErr(err.to_string()))?;

    println!("{}", body);

    send_request(&client, &url, headers, Some(body), "POST")
}

#[automock]
trait FileCreator {
    fn create_file(&self, path: PathBuf) -> Result<Box<dyn FileLike>, CliError>;
}
struct RealFileCreator;
impl FileCreator for RealFileCreator {
    fn create_file(&self, path: PathBuf) -> Result<Box<dyn FileLike>, CliError> {
        let file = File::create(path).map_err(|err| CliError::StatusErr(err.to_string()))?;
        Ok(Box::new(file))
    }
}

struct MockFile {
    content: Vec<u8>,
}

impl MockFile {
    fn new() -> MockFile {
        MockFile { content: Vec::new() }
    }
}
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
#[automock]
trait FileWriter {
    fn write_file(&self, file: &mut Box<(dyn FileLike + 'static)>, content: &[u8]) -> Result<(), CliError>;
}
struct RealFileWriter;
impl FileWriter for RealFileWriter {
    fn write_file(&self,  file: &mut Box<(dyn FileLike + 'static)>, content: &[u8]) -> Result<(), CliError> {
        file.write_all(content)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        Ok(())
    }
}
pub trait FileLike: Read + Write {}

impl FileLike for std::fs::File {}
impl FileLike for MockFile {}

#[cfg(test)]
mod tests {
    use mockall::predicate;

    use super::*;

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
    fn test_create_config_yml(){

            env::set_var("HOME", "/tmp/home");
            let content = "url: http://localhost:8000\ntunnel: TUNNEL_ID\ncredentials-file: /tmp/home/.cloudflared/TUNNEL_ID.json\n";

            let mut file_creator = MockFileCreator::new();
            file_creator.expect_create_file()
                .withf(|path| path.ends_with("config.yml"))
                .returning(|_| Ok(Box::new(MockFile::new()) as Box<dyn FileLike>));
            let mut file_writer = MockFileWriter::new();
            file_writer.expect_write_file()
                .with(predicate::always(), predicate::eq(content.as_bytes()))
                .returning(|_, _| Ok(()));

            let result = create_config_yml(&file_creator,  &file_writer,"TUNNEL_ID");
            assert!(result.is_ok());

            env::remove_var("HOME")
        }
}
