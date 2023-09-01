use crate::{
    background_booting::boot_background_services, start::get_state, stop::shutdown, CliError,
};

pub fn reset() -> Result<(), CliError> {
    // Ensure there is some kind of state from before, otherwise reset doesn't make sense
    get_state()?;

    shutdown()?;
    boot_background_services()
}
