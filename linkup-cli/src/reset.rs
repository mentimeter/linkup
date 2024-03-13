use crate::{
    background_booting::boot_background_services, local_config::LocalState, stop::shutdown,
    CliError,
};

pub fn reset() -> Result<(), CliError> {
    // Ensure there is some kind of state from before, otherwise reset doesn't make sense
    LocalState::load()?;

    shutdown()?;
    boot_background_services()?;

    Ok(())
}
