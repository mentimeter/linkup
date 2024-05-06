use std::{
    env, fs,
    path::{Path, PathBuf},
};

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};

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

pub fn start(config_arg: &Option<String>, no_tunnel: bool) -> Result<(), CliError> {
    let tunnel_id = get_tunnel_id("jumpy-snake")?;
    let tunnel_token = match tunnel_id {
        Some(id) => get_tunnel_token(&id)?,
        None => {
            println!("Creating a new tunnel");
            "".to_string()
            create_tunnel("jumpy-snake")?
        }
    };
    println!("Tunnel token: {}", tunnel_token);
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

fn get_tunnel_id(tunnel_name: &str) -> Result<Option<String>, CliError> {
    let account_id = "set-me";
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
        account_id
    );
    let bearer_token =
        env::var("LINKUP_CF_API_TOKEN").map_err(|err| CliError::BadConfig(err.to_string()))?;

    // Create a client.
    let client = reqwest::blocking::Client::new();

    // Prepare the headers.
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", bearer_token))
            .map_err(|err| CliError::StatusErr(err.to_string()))?,
    );

    // Send the GET request.
    let response = client
        .get(&url)
        .headers(headers)
        .query(&[("name", format!("tunnel-{}", tunnel_name))])
        .send()
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

    // Check if the response status is success and print the response body.
    if response.status().is_success() {
        let response_body = response
            .text()
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        let parsed: GetTunnelApiResponse = serde_json::from_str(&response_body)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        if parsed.result.is_empty() {
            return Ok(None);
        }
        Ok(Some(parsed.result[0].id.clone()))
    } else {
        println!("Failed to get a successful response: {}", response.status());
        Ok(None)
    }
}

fn get_tunnel_token(tunnel_id: &str) -> Result<String, CliError> {
    let account_id = "set-me";
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel/{}/token",
        account_id,
        tunnel_id
    );
    let bearer_token =
        env::var("LINKUP_CF_API_TOKEN").map_err(|err| CliError::BadConfig(err.to_string()))?;

    // Create a client.
    let client = reqwest::blocking::Client::new();

    // Prepare the headers.
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", bearer_token))
            .map_err(|err| CliError::StatusErr(err.to_string()))?,
    );

    // Send the GET request.
    let response = client
        .get(&url)
        .headers(headers)
        .send()
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

    // Check if the response status is success and print the response body.
    if response.status().is_success() {
        let response_body = response
            .text()
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        let parsed: TokenApiResponse = serde_json::from_str(&response_body)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        Ok(parsed.result)
    } else {
        println!("Failed to get a successful response: {}", response.status());
        Err(CliError::StatusErr("Failed to get a successful response".to_string()))
    }
}

fn create_tunnel(tunnel_name: &str) -> Result<String, CliError> {
    let account_id = "set-me";
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/cfd_tunnel",
        account_id,
    );
    let bearer_token =
        env::var("LINKUP_CF_API_TOKEN").map_err(|err| CliError::BadConfig(err.to_string()))?;

    // Create a client.
    let client = reqwest::blocking::Client::new();

    // Prepare the headers.
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", bearer_token))
            .map_err(|err| CliError::StatusErr(err.to_string()))?,
    );

    // create a 32 byte random string and base64 encode it
    // let mut rng = rand::thread_rng();
    // let tunnel_secret: String = (0..32)
    //     .map(|_| rng.gen_range(0..255))
    //     .collect::<Vec<u8>>()
    //     .iter()
    //     .map(|b| *b as char)
    //     .collect();

    let body = serde_json::to_string(&CreateTunnelRequest {
        name: format!("tunnel-{}", tunnel_name),
        tunnel_secret: "AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg=".to_string(),
    }).map_err(|err| CliError::StatusErr(err.to_string()))?;

    // Send the POST request.
    let response = client
        .post(&url)
        .headers(headers)
        .body(body)
        .send()
        .map_err(|err| CliError::StatusErr(err.to_string()))?;

    // Check if the response status is success and print the response body.
    if response.status().is_success() {
        let response_body = response
            .text()
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        let parsed: TunnelResultItem = serde_json::from_str(&response_body)
            .map_err(|err| CliError::StatusErr(err.to_string()))?;
        Ok(parsed.id)
    } else {
        println!("Failed to get a successful response: {}", response.status());
        Err(CliError::StatusErr("Failed to get a successful response".to_string()))
    }
}
