use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::{CliError, start::get_state, local_config::{ServiceTarget, LocalState}, LINKUP_LOCALSERVER_PORT};

#[derive(Deserialize, Serialize)]
struct Status {
  session: SessionStatus,
  linkup: HashMap<String, LinkupStatus>,
  services: HashMap<String, ServiceStatus>,
}

#[derive(Deserialize, Serialize)]
struct SessionStatus {
  session_name: String,
  session_token: String,
  domains: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct LinkupStatus {
  location: String,
  status: String,
}

#[derive(Deserialize, Serialize)]
struct ServiceStatus {
  connected_to: String,
  status: String,
}

pub fn status(json: bool) -> Result<(), CliError> {
    let state = get_state()?;

    let service_statuses = service_status(&state)?; 
    let linkup_statuses = linkup_status(&state);

    let status = Status {
      session: SessionStatus {
        session_name: state.linkup.session_name.clone(),
        session_token: state.linkup.session_token,
        domains: state.domains.iter().map(|d| format!("{}.{}", state.linkup.session_name.clone(), d.domain.clone())).collect(),
      },
      linkup: linkup_statuses,
      services: service_statuses,
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status).unwrap()
        );
    } else {
       // Display session information
       println!("Session Information:");
       println!("  Session Name: {}", status.session.session_name);
       println!("  Session Token: {}", status.session.session_token);
       println!("  Domains: ");
       for domain in &status.session.domains {
           println!("    {}", domain);
       }
       println!();

       // Display linkup information
       println!("Linkup Information:");
       println!("{:<15} {:<15}", "Location", "Status");
       for (location, linkup_status) in &status.linkup {
           println!("{:<15} {:<15}", location, linkup_status.status);
       }
       println!();

       // Display services information
       println!("Services Information:");
       println!("{:<15} {:<15} {:<15}", "Service Name", "Connected To", "Status");
       for (service_name, service_status) in &status.services {
           println!("{:<15} {:<15} {:<15}", service_name, service_status.connected_to, service_status.status);
       }
       println!();
    }

    Ok(())
}

fn linkup_status(state: &LocalState) -> HashMap<String, LinkupStatus> {
  let mut linkup_status_map: HashMap<String, LinkupStatus> = HashMap::new();

  let local_url = format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT);
  linkup_status_map.insert(
    "local server".to_string(),
    LinkupStatus {
      location: local_url.to_string(),
      status: server_status(local_url)
    },
  );

  linkup_status_map.insert(
    "remote server".to_string(),
    LinkupStatus {
      location: state.linkup.remote.to_string(),
      status: server_status(state.linkup.remote.to_string())
    },
  );


  linkup_status_map.insert(
    "tunnel".to_string(),
    LinkupStatus {
      location: state.linkup.tunnel.to_string(),
      status: server_status(state.linkup.tunnel.to_string())
    },
  );

  linkup_status_map
}

fn service_status(state: &LocalState) -> Result<HashMap<String, ServiceStatus>, CliError> {
  let mut service_status_map: HashMap<String, ServiceStatus> = HashMap::new();


    for service in state.services.to_vec() {
        let url = match service.current {
            ServiceTarget::Local => service.local.clone(),
            ServiceTarget::Remote => service.remote.clone(),
        };

        let status = server_status(url.to_string());

        service_status_map.insert(
            service.name,
            ServiceStatus {
                connected_to: service.current.to_string(),
                status,
            },
        );
    }

    Ok(service_status_map)
}

fn server_status(url: String) -> String {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build().unwrap();
      
  let response = client.get(url).send();

  match response {
      Ok(res) if res.status().is_server_error() => "error".to_string(),
      Ok(_) => "ok".to_string(),
      Err(_) => "timeout".to_string(),
  }
}
