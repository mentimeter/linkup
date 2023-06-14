use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::signal::{send_sigint, PidError};
use crate::start::get_state;
use crate::{
    linkup_file_path, CliError, LINKUP_CLOUDFLARED_PID, LINKUP_ENV_SEPARATOR,
    LINKUP_LOCALSERVER_PID_FILE,
};

pub fn stop() -> Result<(), CliError> {
    let local_stopped = match get_pid(LINKUP_LOCALSERVER_PID_FILE) {
        Ok(pid) => send_sigint(&pid).map_err(|e| {
            CliError::StopErr(format!(
                "Could not send SIGINT to local server pid {}: {}",
                pid, e
            ))
        }),
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(CliError::StopErr(format!(
            "Could not get local server pid: {}",
            e
        ))),
    };

    let tunnel_stopped = stop_tunnel();

    let state = get_state()?;
    for service in &state.services {
        let remove_res = match &service.directory {
            Some(d) => remove_service_env(d.clone(), state.linkup.config_path.clone()),
            None => Ok(()),
        };

        if let Err(e) = remove_res {
            println!("Could not remove env for service {}: {}", service.name, e);
        }
    }

    match (local_stopped, tunnel_stopped) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}

pub fn stop_tunnel() -> Result<(), CliError> {
    match get_pid(LINKUP_CLOUDFLARED_PID) {
        Ok(pid) => {
            match send_sigint(&pid) {
                Ok(_) => Ok(()),
                // If we're trying to stop it but it's already died, that's fine
                Err(PidError::NoSuchProcess(_)) => Ok(()),
                Err(e) => Err(CliError::StopErr(format!(
                    "Could not send SIGINT to cloudflared pid {}: {}",
                    pid, e
                ))),
            }
        }
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(CliError::StopErr(format!(
            "Could not get cloudflared pid: {}",
            e
        ))),
    }
}

fn get_pid(file_name: &str) -> Result<String, PidError> {
    if let Err(e) = File::open(linkup_file_path(file_name)) {
        return Err(PidError::NoPidFile(e.to_string()));
    }

    match fs::read_to_string(linkup_file_path(file_name)) {
        Ok(content) => Ok(content.trim().to_string()),
        Err(e) => Err(PidError::BadPidFile(e.to_string())),
    }
}

fn remove_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let env_path = PathBuf::from(config_dir).join(&directory).join(".env");
    let temp_env_path = PathBuf::from(config_dir).join(&directory).join(".env.temp");

    let input_file = File::open(&env_path).map_err(|e| {
        CliError::RemoveServiceEnv(directory.clone(), format!("could not open env file: {}", e))
    })?;
    let reader = BufReader::new(input_file);

    let mut output_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp_env_path)
        .map_err(|e| {
            CliError::RemoveServiceEnv(directory.clone(), format!("could not open env file: {}", e))
        })?;

    let mut copy = true;

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| {
            CliError::RemoveServiceEnv(
                directory.clone(),
                format!("could not read line from env file: {}", e),
            )
        })?;

        if line.trim() == LINKUP_ENV_SEPARATOR {
            copy = !copy;
            continue; // Don't write the separator to the new file
        }

        if copy {
            writeln!(output_file, "{}", line).map_err(|e| {
                CliError::RemoveServiceEnv(
                    directory.clone(),
                    format!("could not write line to env file: {}", e),
                )
            })?;
        }
    }

    fs::rename(&temp_env_path, &env_path).map_err(|e| {
        CliError::RemoveServiceEnv(
            directory.clone(),
            format!("could not set temp env file to master: {}", e),
        )
    })?;

    Ok(())
}
