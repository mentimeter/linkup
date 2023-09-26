use crate::{
    background_booting::boot_background_services,
    linkup_file_path,
    local_config::{config_path, get_config},
    start::{boot_local_dns, get_state},
    stop::shutdown,
    CliError, LINKUP_LOCALDNS_INSTALL,
};

// TODO(ostenbom)[2023-09-26]: Config arg shouldn't be needed here, we could use config state for this
pub fn reset(config_arg: &Option<String>) -> Result<(), CliError> {
    // Ensure there is some kind of state from before, otherwise reset doesn't make sense
    get_state()?;

    shutdown()?;
    boot_background_services()?;

    if linkup_file_path(LINKUP_LOCALDNS_INSTALL).exists() {
        let config_path = config_path(config_arg)?;
        let input_config = get_config(&config_path)?;

        boot_local_dns(&input_config)?;
    }

    Ok(())
}
