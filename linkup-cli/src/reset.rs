use crate::{
    background_booting::boot_background_services,
    linkup_file_path,
    local_config::{config_path, get_config, LocalState},
    start::boot_local_dns,
    stop::shutdown,
    CliError, LINKUP_LOCALDNS_INSTALL,
};

// TODO(ostenbom)[2023-09-26]: Config arg shouldn't be needed here, we could use config state for this
pub fn reset(config_arg: &Option<String>) -> Result<(), CliError> {
    // Ensure there is some kind of state from before, otherwise reset doesn't make sense
    LocalState::load()?;

    shutdown()?;
    boot_background_services()?;

    if linkup_file_path(LINKUP_LOCALDNS_INSTALL).exists() {
        let config_path = config_path(config_arg)?;
        let input_config = get_config(&config_path)?;

        boot_local_dns(&input_config)?;
    }

    Ok(())
}
