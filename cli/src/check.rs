use std::{path::PathBuf, fs, env};

use crate::{CliError, local_config::LocalState};


pub fn save_state(state: LocalState) -> Result<(), CliError> {
  let yaml_string = match serde_yaml::to_string(&state) {
      Ok(yaml) => yaml,
      Err(_) => {
          return Err(CliError::SaveState(
              "Failed to serialize the state into YAML".to_string(),
          ))
      }
  };

  let home_dir = match env::var("HOME") {
      Ok(val) => val,
      Err(_) => {
          return Err(CliError::SaveState(
              "Failed to get the HOME environment variable".to_string(),
          ))
      }
  };

  let mut path = PathBuf::from(home_dir);
  path.push(".serpress");

  if let Err(_) = fs::write(&path, yaml_string) {
      return Err(CliError::SaveState(format!(
          "Failed to write the state file at {}",
          path.display()
      )));
  }

  Ok(())
}