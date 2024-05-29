use crate::{
    background_booting::{BackgroundServices, RealBackgroundServices},
    local_config::LocalState,
    stop::shutdown,
    CliError,
};

pub fn reset() -> Result<(), CliError> {
    // Ensure there is some kind of state from before, otherwise reset doesn't make sense
    let state = LocalState::load()?;

    shutdown()?;
    let background_service = RealBackgroundServices;
    let _ = background_service.boot_background_services(state);

    Ok(())
}
