use url::Url;

use crate::{
    background_booting::{load_config, server_config_from_state},
    local_config::{LocalState, ServiceTarget},
    start::{get_state, save_state},
    CliError, LINKUP_LOCALSERVER_PORT,
};

pub fn remote(service_names: &[String]) -> Result<(), CliError> {
    if service_names.is_empty() {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }
    let mut state = get_state()?;

    for service_name in service_names.clone() {
        let service = state
            .services
            .iter_mut()
            .find(|s| s.name.as_str() == service_name)
            .ok_or_else(|| CliError::NoSuchService(service_name.to_string()))?;
        service.current = ServiceTarget::Remote;
    }

    save_state(state.clone())?;
    load_server_states(state)?;

    println!(
        "Linkup is routing {} traffic to the remote server",
        service_names.join(", ")
    );

    Ok(())
}

pub fn local(service_names: &[String]) -> Result<(), CliError> {
    if service_names.is_empty() {
        return Err(CliError::NoSuchService(
            "No service names provided".to_string(),
        ));
    }

    let mut state = get_state()?;

    for service_name in service_names.clone() {
        let service = state
            .services
            .iter_mut()
            .find(|s| s.name.as_str() == service_name)
            .ok_or_else(|| CliError::NoSuchService(service_name.to_string()))?;
        service.current = ServiceTarget::Local;
    }

    save_state(state.clone())?;
    load_server_states(state)?;

    println!(
        "Linkup is routing {} traffic to the local server",
        service_names.join(", ")
    );

    Ok(())
}

fn load_server_states(state: LocalState) -> Result<(), CliError> {
    let local_url = Url::parse(&format!("http://localhost:{}", LINKUP_LOCALSERVER_PORT))
        .expect("linkup url invalid");

    let (local_server_conf, remote_server_conf) = server_config_from_state(&state);
    let _ = load_config(
        &state.linkup.remote,
        &state.linkup.session_name.clone(),
        remote_server_conf,
    )?;
    let _ = load_config(&local_url, &state.linkup.session_name, local_server_conf)?;

    Ok(())
}
