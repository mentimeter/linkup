use std::{
    env,
    fs::{self, File},
};

use crate::{
    check::check,
    linkup_file_path,
    local_config::{config_to_state, LocalState, YamlLocalConfig},
    CliError, LINKUP_CONFIG_ENV, LINKUP_STATE_FILE,
};

pub fn start(config_arg: Option<String>) -> Result<(), CliError> {
    // TODO: run `stop` to kill the previous local server?

    let previous_state = get_state();
    let input_config = get_config(config_arg)?;

    let mut state = config_to_state(input_config);

    // Reuse previous session name if possible
    if let Ok(ps) = previous_state {
        state.linkup.session_name = ps.linkup.session_name;
        state.linkup.session_token = ps.linkup.session_token;
    }

    save_state(state)?;

    check()?;

    Ok(())
}

fn get_config(config_arg: Option<String>) -> Result<YamlLocalConfig, CliError> {
    let config_path =
        match config_arg {
            Some(path) => path,
            None => match env::var(LINKUP_CONFIG_ENV) {
                Ok(val) => val,
                Err(_) => return Err(CliError::BadConfig(
                    "No config argument provided and LINKUP_CONFIG environment variable not set"
                        .to_string(),
                )),
            },
        };

    let content = match fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(_) => {
            return Err(CliError::BadConfig(format!(
                "Failed to read the config file at {}",
                config_path
            )))
        }
    };

    let yaml_config: YamlLocalConfig = match serde_yaml::from_str(&content) {
        Ok(config) => config,
        Err(_) => {
            return Err(CliError::BadConfig(format!(
                "Failed to deserialize the config file at {}",
                config_path
            )))
        }
    };

    Ok(yaml_config)
}

pub fn get_state() -> Result<LocalState, CliError> {
    if let Err(e) = File::open(&linkup_file_path(LINKUP_STATE_FILE)) {
        return Err(CliError::NoState(e.to_string()));
    }

    let content = match fs::read_to_string(&linkup_file_path(LINKUP_STATE_FILE)) {
        Ok(content) => content,
        Err(e) => return Err(CliError::NoState(e.to_string())),
    };

    match serde_yaml::from_str(&content) {
        Ok(config) => Ok(config),
        Err(e) => Err(CliError::NoState(e.to_string())),
    }
}

pub fn save_state(state: LocalState) -> Result<(), CliError> {
    let yaml_string = match serde_yaml::to_string(&state) {
        Ok(yaml) => yaml,
        Err(_) => {
            return Err(CliError::SaveState(
                "Failed to serialize the state into YAML".to_string(),
            ))
        }
    };

    if let Err(_) = fs::write(&linkup_file_path(LINKUP_STATE_FILE), yaml_string) {
        return Err(CliError::SaveState(format!(
            "Failed to write the state file at {}",
            linkup_file_path(LINKUP_STATE_FILE).display()
        )));
    }

    Ok(())
}
