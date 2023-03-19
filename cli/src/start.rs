use std::{env, fs::{self, File}, path::PathBuf};

use crate::{local_config::{config_to_state, YamlLocalConfig, LocalState}, check::save_state, CliError};

pub fn start(config_arg: Option<String>) -> Result<(), CliError> {
  let previous_state = get_state();
  let input_config = get_config(config_arg)?;

  let mut state = config_to_state(input_config);

  // Reuse previous session name if possible
  if let Ok(ps) = previous_state {
      state.serpress.session_name = ps.serpress.session_name
  }

  save_state(state)?;

  Ok(())
}

fn get_config(config_arg: Option<String>) -> Result<YamlLocalConfig, CliError> {
  let config_path = match config_arg {
      Some(path) => path,
      None => match env::var("SERPRESS_CONFIG") {
          Ok(val) => val,
          Err(_) => {
              return Err(CliError::BadConfig(
                  "No config argument provided and SERPRESS_CONFIG environment variable not set"
                      .to_string(),
              ))
          }
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

fn get_state() -> Result<LocalState, CliError> {
  let home_dir = match env::var("HOME") {
      Ok(val) => val,
      Err(e) => return Err(CliError::NoState(e.to_string())),
  };

  let mut path = PathBuf::from(home_dir);
  path.push(".serpress");

  if let Err(e) = File::open(&path) {
    return Err(CliError::NoState(e.to_string()));
  }

  let content = match fs::read_to_string(&path) {
      Ok(content) => content,
      Err(e) => return Err(CliError::NoState(e.to_string())),
  };

  match serde_yaml::from_str(&content) {
      Ok(config) => Ok(config),
      Err(e) => return Err(CliError::NoState(e.to_string())),
  }
}