use crate::{
    background_booting::boot_background_services,
    local_config::ServiceTarget,
    start::{get_state, save_state},
    CliError,
};

pub fn remote(service_name: String) -> Result<(), CliError> {
    let mut state = get_state()?;

    let mut service = state
        .services
        .iter_mut()
        .find(|s| s.name == service_name)
        .ok_or(CliError::BadConfig(format!(
            "Service {} not found",
            service_name
        )))?;
    service.current = ServiceTarget::Remote;

    save_state(state)?;
    boot_background_services()?;

    Ok(())
}

pub fn local(service_name: String) -> Result<(), CliError> {
    let mut state = get_state()?;

    let mut service = state
        .services
        .iter_mut()
        .find(|s| s.name == service_name)
        .ok_or(CliError::BadConfig(format!(
            "Service {} not found",
            service_name
        )))?;
    service.current = ServiceTarget::Local;

    save_state(state)?;
    boot_background_services()?;
    Ok(())
}
