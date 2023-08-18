use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use reqwest::StatusCode;

use linkup::{StorableService, StorableSession, UpdateSessionRequest};
use url::Url;

use crate::background_local_server::{
    is_local_server_started, is_tunnel_started, start_local_server,
};
use crate::background_tunnel::start_tunnel;
use crate::local_config::{LocalState, ServiceTarget};
use crate::start::save_state;
use crate::status::print_session_names;
use crate::{start::get_state, CliError};
use crate::{LINKUP_ENV_SEPARATOR, LINKUP_LOCALSERVER_PORT};

pub fn boot_background_services() -> Result<(), CliError> {
    let mut state = get_state()?;

    let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
        .expect("linkup url invalid");

    if is_local_server_started().is_err() {
        println!("Starting linkup local server...");
        start_local_server()?;
    } else {
        println!("Linkup local server was already running.. Try stopping linkup first if you have problems.");
    }

    wait_till_ok(format!("{}linkup-check", local_url))?;

    if is_tunnel_started().is_err() {
        println!("Starting tunnel...");
        let tunnel = start_tunnel()?;
        state.linkup.tunnel = tunnel;
    } else {
        println!("Cloudflare tunnel was already running.. Try stopping linkup first if you have problems.");
    }

    for service in &state.services {
        match &service.directory {
            Some(d) => set_service_env(d.clone(), state.linkup.config_path.clone())?,
            None => {}
        }
    }

    let (local_server_conf, remote_server_conf) = server_config_from_state(&state);

    let server_session_name = load_config(
        &state.linkup.remote,
        &state.linkup.session_name,
        remote_server_conf,
    )?;
    let local_session_name = load_config(&local_url, &server_session_name, local_server_conf)?;

    if server_session_name != local_session_name {
        return Err(CliError::InconsistentState);
    }

    let tunnel_url = state.linkup.tunnel.clone();

    state.linkup.session_name = server_session_name.clone();
    let state_to_print = state.clone();

    save_state(state)?;

    println!("Waiting for tunnel to be ready at {}...", tunnel_url);

    // If the tunnel is checked too quickly, it dies ¯\_(ツ)_/¯
    thread::sleep(Duration::from_millis(1000));
    wait_till_ok(format!("{}linkup-check", tunnel_url))?;

    println!();

    print_session_names(&state_to_print);

    Ok(())
}

pub fn load_config(
    url: &Url,
    desired_name: &str,
    config: StorableSession,
) -> Result<String, CliError> {
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

pub fn server_config_from_state(state: &LocalState) -> (StorableSession, StorableSession) {
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

pub fn wait_till_ok(url: String) -> Result<(), CliError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .map_err(|err| CliError::StartLinkupTimeout(err.to_string()))?;

    let start = Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(20) {
            return Err(CliError::StartLinkupTimeout(format!(
                "{} took too long to load",
                url
            )));
        }

        let response = client.get(&url).send();

        if let Ok(resp) = response {
            if resp.status() == StatusCode::OK {
                return Ok(());
            }
        }

        thread::sleep(Duration::from_millis(2000));
    }
}

fn set_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let dev_env_files_result = fs::read_dir(&service_path);
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

        if let Ok(env_content) = fs::read_to_string(&env_path) {
            if env_content.contains(LINKUP_ENV_SEPARATOR) {
                continue;
            }
        }

        let dev_env_content = fs::read_to_string(&dev_env_path).map_err(|e| {
            CliError::SetServiceEnv(
                directory.clone(),
                format!("could not read dev env file: {}", e),
            )
        })?;

        let mut env_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&env_path)
            .map_err(|e| {
                CliError::SetServiceEnv(
                    directory.clone(),
                    format!("Failed to open .env file: {}", e),
                )
            })?;

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
    }

    Ok(())
}
