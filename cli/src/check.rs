
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde_yaml;

use serpress::{YamlServerConfig, YamlServerService, YamlServerConfigPost};
use url::Url;

use crate::SERPRESS_PORT;
use crate::background_services::{is_local_server_started, start_local_server, is_tunnel_started, start_tunnel};
use crate::local_config::{LocalState, ServiceTarget};
use crate::start::save_state;
use crate::{CliError, start::get_state};


pub fn check() -> Result<(), CliError> {
  let mut state = get_state()?;

  if let Err(_) = is_local_server_started() {
    start_local_server()?
  }

  if let Err(_) = is_tunnel_started() {
    let tunnel = start_tunnel()?;
    state.serpress.tunnel = tunnel;
  }

  let (local_server_conf, remote_server_conf) = server_config_from_state(&state);
  let localUrl = Url::parse(&format!("http://localhost:{}", SERPRESS_PORT)).expect("serpress url invalid");

  let server_session_name = load_config(&state.serpress.remote, &state.serpress.session_name, remote_server_conf)?;
  let local_session_name = load_config(&localUrl, &server_session_name, local_server_conf)?;

  if server_session_name != local_session_name {
    return Err(CliError::InconsistentState)
  }

  state.serpress.session_name = server_session_name;
  save_state(state)?;

  // final checks services are responding
  // print status

  Ok(())
}

fn load_config(url: &Url, desired_name: &String, config: YamlServerConfig) -> Result<String, CliError> {
  let client = Client::new();
    let endpoint = url.join("/serpress").map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let config_post = YamlServerConfigPost {
        desired_name: desired_name.clone(),
        services: config.services,
        domains: config.domains,
    };

    let config_post_yaml = serde_yaml::to_string(&config_post).map_err(|e| CliError::LoadConfig(url.to_string(), e.to_string()))?;

    let response = client
        .post(endpoint)
        .body(config_post_yaml)
        .send()
        .map_err(|e| CliError::LoadConfig(desired_name.clone(), e.to_string()))?;

    match response.status() {
        StatusCode::OK => {
            let content = String::new();
            response.text().map_err(|e| CliError::LoadConfig(desired_name.clone(), e.to_string()))?;
            Ok(content)
        }
        _ => Err(CliError::LoadConfig(url.to_string(), format!("status code: {}", response.status()))),
    }
}

fn server_config_from_state(state: &LocalState) -> (YamlServerConfig, YamlServerConfig) {
  let local_server_services = state.services
      .iter()
      .map(|local_service| YamlServerService {
          name: local_service.name.clone(),
          location: if local_service.current == ServiceTarget::Remote {
              local_service.remote.clone()
          } else {
              local_service.local.clone()
          },
          path_modifiers: Some(local_service.path_modifiers.clone()),
      })
      .collect::<Vec<YamlServerService>>();

  let remote_server_services = state.services
      .iter()
      .map(|local_service| YamlServerService {
          name: local_service.name.clone(),
          location: if local_service.current == ServiceTarget::Remote {
              local_service.remote.clone()
          } else {
              state.serpress.tunnel.clone()
          },
          path_modifiers: Some(local_service.path_modifiers.clone()),
      })
      .collect::<Vec<YamlServerService>>();

  (
      YamlServerConfig {
          services: local_server_services,
          domains: state.domains.clone(),
      },
      YamlServerConfig {
          services: remote_server_services,
          domains: state.domains.clone(),
      },
  )
}





