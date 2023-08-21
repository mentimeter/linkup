use crate::{start, stop, CliError};

pub fn reset(config_arg: Option<String>) -> Result<(), CliError> {
    stop()?;
    start(config_arg)
}
