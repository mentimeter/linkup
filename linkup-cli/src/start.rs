use std::{
    env, fs,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use reqwest::{
    header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    Method,
};

use crate::env_files::write_to_env_file;
use crate::local_config::{config_path, get_config};
use crate::LINKUP_LOCALDNS_INSTALL;
use crate::{
    background_booting::boot_background_services,
    linkup_file_path,
    local_config::{config_to_state, LocalState},
    status::{server_status, ServerStatus},
    CliError,
};
use serde::{Deserialize, Serialize};
use serde_yaml;

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

fn file_exists(file_path: &str) -> bool {
    Path::new(file_path).exists()
}

pub fn start(config_arg: &Option<String>, no_tunnel: bool) -> Result<(), CliError> {
    let tunnel_name = "happy-lion".to_string();
    let tunnel_id = match get_tunnel_id(&tunnel_name) {
        Ok(Some(id)) => id,
        Ok(None) => "".to_string(),
        Err(e) => return Err(e),
    };

    // If there exists a /$ENV_HOME/.cloudflared/<Tunnel-UUID>.json file, skip creating a tunnel
    let file_path = format!(
        "{}/.cloudflared/{}.json",
        env::var("HOME").expect("HOME is not set"),
        tunnel_id
    );
    if file_exists(&file_path) {
        println!("File exists: {}", file_path);
    } else {
        println!("File does not exist: {}", file_path);
        let tunnel_id = create_tunnel(&tunnel_name)?;
        create_dns_record(&tunnel_id, &tunnel_name)?;
    }

    Ok(())
    // let previous_state = LocalState::load();
    // let config_path = config_path(config_arg)?;
    // let input_config = get_config(&config_path)?;

    // let mut state = config_to_state(input_config.clone(), config_path, no_tunnel);

    // // Reuse previous session name if possible
    // if let Ok(ps) = previous_state {
    //     state.linkup.session_name = ps.linkup.session_name;
    //     state.linkup.session_token = ps.linkup.session_token;

    //     // Maintain tunnel state until it is rewritten
    //     if !no_tunnel && ps.linkup.tunnel.is_some() {
    //         state.linkup.tunnel = ps.linkup.tunnel;
    //     }
    // }

    // state.save()?;

    // // Set env vars to linkup
    // for service in &state.services {
    //     match &service.directory {
    //         Some(d) => set_service_env(d.clone(), state.linkup.config_path.clone())?,
    //         None => {}
    //     }
    // }

    // if no_tunnel && !linkup_file_path(LINKUP_LOCALDNS_INSTALL).exists() {
    //     println!("Run `linkup local-dns install` before running without a tunnel");

    //     return Err(CliError::NoTunnelWithoutLocalDns);
    // }

    // boot_background_services()?;

    // check_local_not_started()?;

    // Ok(())
}

fn set_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let dev_env_files_result = fs::read_dir(service_path);
    let dev_env_files: Vec<_> = match dev_env_files_result {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().ends_with(".linkup")
                    && entry.file_name().to_string_lossy().starts_with(".env.")
            })
            .collect(),
        Err(e) => {
            return Err(CliError::SetServiceEnv(
                directory.clone(),
                format!("Failed to read directory: {}", e),
            ))
        }
    };

    if dev_env_files.is_empty() {
        return Err(CliError::NoDevEnv(directory));
    }

    for dev_env_file in dev_env_files {
        let dev_env_path = dev_env_file.path();
        let env_path =
            PathBuf::from(dev_env_path.parent().unwrap()).join(dev_env_path.file_stem().unwrap());

        write_to_env_file(&directory, &dev_env_path, &env_path)?;
    }

    Ok(())
}

fn check_local_not_started() -> Result<(), CliError> {
    let state = LocalState::load()?;
    for service in state.services {
        if service.local == service.remote {
            continue;
        }
        if server_status(service.local.to_string()) == ServerStatus::Ok {
            println!("⚠️  Service {} is already running locally!! You need to restart it for linkup's environment variables to be loaded.", service.name);
        }
    }
    Ok(())
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

fn get_tunnel_id(tunnel_name: &str) -> Result<Option<String>, CliError> {
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

fn create_tunnel(tunnel_name: &str) -> Result<String, CliError> {
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
    create_config_yml(&parsed.result.id).map_err(|err| CliError::StatusErr(err.to_string()))?;

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

fn create_config_yml(tunnel_id: &str) -> Result<(), CliError> {
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

    let mut file = File::create(dir_path.join("config.yml"))
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    file.write_all(serialized.as_bytes())
        .map_err(|err| CliError::StatusErr(err.to_string()))?;
    Ok(())
}

fn create_dns_record(tunnel_id: &str, tunnel_name: &str) -> Result<(), CliError> {
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
