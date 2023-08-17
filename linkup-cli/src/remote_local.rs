use url::Url;

use crate::{
    background_booting::{load_config, server_config_from_state},
    local_config::{LocalState, ServiceTarget},
    start::{get_state, save_state},
    CliError, LINKUP_LOCALSERVER_PORT,
};

pub fn remote(service_name: String) -> Result<(), CliError> {
    let mut state = get_state()?;

    let service = state
        .services
        .iter_mut()
        .find(|s| s.name == service_name)
        .ok_or(CliError::BadConfig(format!(
            "Service {} not found",
            service_name
        )))?;
    service.current = ServiceTarget::Remote;

    save_state(state.clone())?;
    load_server_states(state)?;

    println!(
        "Linkup is routing {} traffic to the remote server",
        service_name
    );

    Ok(())
}

pub fn local(service_name: String) -> Result<(), CliError> {
    let mut state = get_state()?;

    let service = state
        .services
        .iter_mut()
        .find(|s| s.name == service_name)
        .ok_or(CliError::BadConfig(format!(
            "Service {} not found",
            service_name
        )))?;
    service.current = ServiceTarget::Local;

    save_state(state.clone())?;
    load_server_states(state)?;

    println!(
        "Linkup is routing {} traffic to the local server",
        service_name
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
