use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use reqwest::blocking::Client;
use reqwest::StatusCode;

use linkup::{StorableService, StorableSession, UpdateSessionRequest};
use url::Url;

use crate::background_services::{
    is_local_server_started, is_tunnel_started, start_local_server, start_tunnel,
};
use crate::local_config::{LocalState, ServiceTarget};
use crate::start::save_state;
use crate::{start::get_state, CliError};
use crate::{LINKUP_ENV_SEPARATOR, LINKUP_LOCALSERVER_PORT};

pub fn check() -> Result<(), CliError> {
    let mut state = get_state()?;

    if is_local_server_started().is_err() {
        start_local_server()?;
    }

    if is_tunnel_started().is_err() {
        let tunnel = start_tunnel()?;
        state.linkup.tunnel = tunnel;
    }

    for service in &state.services {
        match &service.directory {
            Some(d) => set_service_env(d.clone(), state.linkup.config_path.clone())?,
            None => {}
        }
    }

    let (local_server_conf, remote_server_conf) = server_config_from_state(&state);
    let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
        .expect("linkup url invalid");

    let server_session_name = load_config(
        &state.linkup.remote,
        &state.linkup.session_name,
        remote_server_conf,
    )?;
    let local_session_name = load_config(&local_url, &server_session_name, local_server_conf)?;

    if server_session_name != local_session_name {
        return Err(CliError::InconsistentState);
    }

    state.linkup.session_name = server_session_name.clone();
    save_state(state)?;

    println!("{}", server_session_name);

    // final checks services are responding
    // print status

    Ok(())
}

fn load_config(url: &Url, desired_name: &str, config: StorableSession) -> Result<String, CliError> {
    let client = Client::new();
    let endpoint = url
        .join("/linkup")
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let session_update_req = UpdateSessionRequest {
        session_token: config.session_token,
        desired_name: desired_name.into(),
        services: config.services,
        domains: config.domains,
        cache_routes: config.cache_routes,
    };

    let update_req_json = serde_json::to_string(&session_update_req)
        .map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let response = client
        .post(endpoint.clone())
        .body(update_req_json)
        .send()
        .map_err(|e| CliError::LoadConfig(desired_name.into(), e.to_string()))?;

    match response.status() {
        StatusCode::OK => {
            let content = response
                .text()
                .map_err(|e| CliError::LoadConfig(desired_name.into(), e.to_string()))?;
            Ok(content)
        }
        _ => Err(CliError::LoadConfig(
            endpoint.to_string(),
            format!("status code: {}", response.status()),
        )),
    }
}

fn server_config_from_state(state: &LocalState) -> (StorableSession, StorableSession) {
    let local_server_services = state
        .services
        .iter()
        .map(|local_service| StorableService {
            name: local_service.name.clone(),
            location: if local_service.current == ServiceTarget::Remote {
                local_service.remote.clone()
            } else {
                local_service.local.clone()
            },
            rewrites: Some(local_service.rewrites.clone()),
        })
        .collect::<Vec<StorableService>>();

    let remote_server_services = state
        .services
        .iter()
        .map(|local_service| StorableService {
            name: local_service.name.clone(),
            location: if local_service.current == ServiceTarget::Remote {
                local_service.remote.clone()
            } else {
                state.linkup.tunnel.clone()
            },
            rewrites: Some(local_service.rewrites.clone()),
        })
        .collect::<Vec<StorableService>>();

    (
        StorableSession {
            session_token: state.linkup.session_token.clone(),
            services: local_server_services,
            domains: state.domains.clone(),
            cache_routes: state.linkup.cache_routes.clone(),
        },
        StorableSession {
            session_token: state.linkup.session_token.clone(),
            services: remote_server_services,
            domains: state.domains.clone(),
            cache_routes: state.linkup.cache_routes.clone(),
        },
    )
}

fn set_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let dev_env_path = PathBuf::from(config_dir).join(&directory).join(".env.dev");
    let env_path = PathBuf::from(config_dir).join(&directory).join(".env");

    if !Path::new(&dev_env_path).exists() {
        return Err(CliError::NoDevEnv(directory));
    }

    let dev_env_content = fs::read_to_string(&dev_env_path).map_err(|e| {
        CliError::SetServiceEnv(
            directory.clone(),
            format!("could not read dev env file: {}", e),
        )
    })?;

    if let Ok(env_content) = fs::read_to_string(&env_path) {
        if env_content.contains(LINKUP_ENV_SEPARATOR) {
            return Ok(());
        }
    }

    let mut env_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(env_path)
        .map_err(|e| CliError::SetServiceEnv(directory.clone(), e.to_string()))?;

    writeln!(env_file, "{}", LINKUP_ENV_SEPARATOR).map_err(|e| {
        CliError::SetServiceEnv(
            directory.clone(),
            format!("could not write to env file: {}", e),
        )
    })?;

    writeln!(env_file, "{}", dev_env_content).map_err(|e| {
        CliError::SetServiceEnv(
            directory.clone(),
            format!("could not write to env file: {}", e),
        )
    })?;

    writeln!(env_file, "{}", LINKUP_ENV_SEPARATOR).map_err(|e| {
        CliError::SetServiceEnv(
            directory.clone(),
            format!("could not write to env file: {}", e),
        )
    })?;

    Ok(())
}
