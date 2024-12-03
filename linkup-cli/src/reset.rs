use crate::{
    background_booting::{BackgroundServices, LocalBackgroundServices},
    local_config::LocalState,
    stop::shutdown,
    CliError,
};

pub fn reset() -> Result<(), CliError> {
    // Ensure there is some kind of state from before, otherwise reset doesn't make sense
    let state = LocalState::load()?;

    shutdown()?;
    let background_service = LocalBackgroundServices;
    let _ = background_service.boot_linkup_server(state);

    Ok(())
}
