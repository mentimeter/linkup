use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};

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

    let tunnel_stopped = match get_pid(LINKUP_CLOUDFLARED_PID) {
        Ok(pid) => send_sigint(&pid).map_err(|e| {
            CliError::StopErr(format!(
                "Could not send SIGINT to cloudflared pid {}: {}",
                pid, e
            ))
        }),
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(CliError::StopErr(format!(
            "Could not get cloudflared pid: {}",
            e
        ))),
    };

    let state = get_state()?;
    for service in &state.services {
        match &service.directory {
            Some(d) => remove_service_env(d.clone())?,
            None => {}
        }
    }

    match (local_stopped, tunnel_stopped) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
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

fn remove_service_env(directory: String) -> Result<(), CliError> {
    let env_path = format!("{}/.env", directory);
    let temp_env_path = format!("{}/.env.temp", directory);

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
