use std::io::Write;
use std::{
    env,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::{
    background_booting::boot_background_services,
    linkup_file_path,
    local_config::{config_to_state, LocalState, YamlLocalConfig},
    status::{server_status, ServerStatus},
    CliError, LINKUP_CONFIG_ENV, LINKUP_ENV_SEPARATOR, LINKUP_STATE_FILE,
};

pub fn start(config_arg: Option<String>) -> Result<(), CliError> {
    let previous_state = get_state();
    let config_path = config_path(config_arg)?;
    let input_config = get_config(config_path.clone())?;

    let mut state = config_to_state(input_config, config_path);

    // Reuse previous session name if possible
    if let Ok(ps) = previous_state {
        state.linkup.session_name = ps.linkup.session_name;
        state.linkup.session_token = ps.linkup.session_token;

        // Maintain tunnel state until it is rewritten
        state.linkup.tunnel = ps.linkup.tunnel;
    }

    save_state(state.clone())?;

    // Set env vars to linkup
    for service in &state.services {
        match &service.directory {
            Some(d) => set_service_env(d.clone(), state.linkup.config_path.clone())?,
            None => {}
        }
    }

    boot_background_services()?;

    check_local_not_started()?;

    Ok(())
}

fn config_path(config_arg: Option<String>) -> Result<String, CliError> {
    match config_arg {
        Some(path) => {
            let absolute_path = fs::canonicalize(path)
                .map_err(|_| CliError::NoConfig("Unable to resolve absolute path".to_string()))?;
            Ok(absolute_path.to_string_lossy().into_owned())
        }
        None => match env::var(LINKUP_CONFIG_ENV) {
            Ok(val) => {
                let absolute_path = fs::canonicalize(val).map_err(|_| {
                    CliError::NoConfig("Unable to resolve absolute path".to_string())
                })?;
                Ok(absolute_path.to_string_lossy().into_owned())
            }
            Err(_) => Err(CliError::NoConfig(
                "No config argument provided and LINKUP_CONFIG environment variable not set"
                    .to_string(),
            )),
        },
    }
}

fn get_config(config_path: String) -> Result<YamlLocalConfig, CliError> {
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
    if let Err(e) = File::open(linkup_file_path(LINKUP_STATE_FILE)) {
        return Err(CliError::NoState(e.to_string()));
    }

    let content = match fs::read_to_string(linkup_file_path(LINKUP_STATE_FILE)) {
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

    if fs::write(linkup_file_path(LINKUP_STATE_FILE), yaml_string).is_err() {
        return Err(CliError::SaveState(format!(
            "Failed to write the state file at {}",
            linkup_file_path(LINKUP_STATE_FILE).display()
        )));
    }

    Ok(())
}

fn set_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let dev_env_files_result = fs::read_dir(&service_path);
    let dev_env_files: Vec<_> = match dev_env_files_result {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_name().to_string_lossy().ends_with(".linkup")
                    && entry.file_name().to_string_lossy().starts_with(".env.")
            })
            .collect(),
        Err(e) => {
            return Err(CliError::SetServiceEnv(
                directory.clone(),
                format!("Failed to read directory: {}", e),
            ))
        }
    };

    if dev_env_files.is_empty() {
        return Err(CliError::NoDevEnv(directory));
    }

    for dev_env_file in dev_env_files {
        let dev_env_path = dev_env_file.path();
        let env_path =
            PathBuf::from(dev_env_path.parent().unwrap()).join(dev_env_path.file_stem().unwrap());

        if let Ok(env_content) = fs::read_to_string(&env_path) {
            if env_content.contains(LINKUP_ENV_SEPARATOR) {
                continue;
            }
        }

        let dev_env_content = fs::read_to_string(&dev_env_path).map_err(|e| {
            CliError::SetServiceEnv(
                directory.clone(),
                format!("could not read dev env file: {}", e),
            )
        })?;

        let mut env_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&env_path)
            .map_err(|e| {
                CliError::SetServiceEnv(
                    directory.clone(),
                    format!("Failed to open .env file: {}", e),
                )
            })?;

        writeln!(env_file, "{}", LINKUP_ENV_SEPARATOR).map_err(|e| {
            CliError::SetServiceEnv(
                directory.clone(),
                format!("could not write to env file: {}", e),
            )
        })?;

        writeln!(env_file, "{}", dev_env_content).map_err(|e| {
            CliError::SetServiceEnv(
                directory.clone(),
                format!("could not write to env file: {}", e),
            )
        })?;

        writeln!(env_file, "{}", LINKUP_ENV_SEPARATOR).map_err(|e| {
            CliError::SetServiceEnv(
                directory.clone(),
                format!("could not write to env file: {}", e),
            )
        })?;
    }

    Ok(())
}

fn check_local_not_started() -> Result<(), CliError> {
    let state = get_state()?;
    for service in state.services {
        if service.local == service.remote {
            continue;
        }
        if server_status(service.local.to_string()) == ServerStatus::Ok {
            println!("⚠️  Service {} is already running locally!! You need to restart it for linkup's environment variables to be loaded.", service.name);
        }
    }
    Ok(())
}
