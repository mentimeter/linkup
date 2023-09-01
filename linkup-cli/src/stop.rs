use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::signal::{send_sigint, PidError};
use crate::start::get_state;
use crate::{
    linkup_file_path, CliError, LINKUP_CLOUDFLARED_PID, LINKUP_ENV_SEPARATOR,
    LINKUP_LOCALSERVER_PID_FILE,
};

pub fn stop() -> Result<(), CliError> {
    let local_stopped = stop_pid_file(LINKUP_LOCALSERVER_PID_FILE);
    if local_stopped.is_ok() {
        let _ = std::fs::remove_file(linkup_file_path(LINKUP_LOCALSERVER_PID_FILE));
    }
    let tunnel_stopped = stop_pid_file(LINKUP_CLOUDFLARED_PID);
    if tunnel_stopped.is_ok() {
        let _ = std::fs::remove_file(linkup_file_path(LINKUP_CLOUDFLARED_PID));
    }

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
        (Ok(_), Ok(_)) => {
            println!("Stopped linkup");
            Ok(())
        }
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}

pub fn stop_pid_file(pid_file: &str) -> Result<(), CliError> {
    match get_pid(pid_file) {
        Ok(pid) => {
            match send_sigint(&pid) {
                Ok(_) => Ok(()),
                // If we're trying to stop it but it's already died, that's fine
                Err(PidError::NoSuchProcess(_)) => Ok(()),
                Err(e) => Err(CliError::StopErr(format!(
                    "Could not send SIGINT to {} pid {}: {}",
                    pid_file, pid, e
                ))),
            }
        }
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(CliError::StopErr(format!(
            "Could not get {} pid: {}",
            pid_file, e
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

    let service_path = PathBuf::from(config_dir).join(&directory);

    let env_files_result = fs::read_dir(&service_path);
    let env_files: Vec<_> = match env_files_result {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(".env"))
            .collect(),
        Err(e) => {
            return Err(CliError::SetServiceEnv(
                directory.clone(),
                format!("Failed to read directory: {}", e),
            ))
        }
    };

    for env_file in env_files {
        let env_path = env_file.path();

        let mut file_content = fs::read_to_string(&env_path).map_err(|e| {
            CliError::RemoveServiceEnv(
                directory.clone(),
                format!("could not read dev env file: {}", e),
            )
        })?;

        let start_idx = file_content.find(LINKUP_ENV_SEPARATOR);
        let end_idx = file_content.rfind(LINKUP_ENV_SEPARATOR);

        if let (Some(start), Some(end)) = (start_idx, end_idx) {
            if start < end {
                file_content.drain(start..=end + LINKUP_ENV_SEPARATOR.len() - 1);
            }
            if file_content.ends_with('\n') {
                file_content.pop();
            }

            // Write the updated content back to the file
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&env_path)
                .map_err(|e| {
                    CliError::RemoveServiceEnv(
                        directory.clone(),
                        format!("Failed to open .env file for writing: {}", e),
                    )
                })?;
            file.write_all(file_content.as_bytes()).map_err(|e| {
                CliError::RemoveServiceEnv(
                    directory.clone(),
                    format!("Failed to write .env file: {}", e),
                )
            })?;
        }
    }

    Ok(())
}
