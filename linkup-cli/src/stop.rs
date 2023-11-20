use std::fs::{self};
use std::path::{Path, PathBuf};

use nix::sys::signal::Signal;

use crate::env_files::clear_env_file;
use crate::signal::{get_pid, send_signal, PidError};
use crate::start::get_state;
use crate::{
    linkup_file_path, services, CliError, LINKUP_CLOUDFLARED_PID, LINKUP_LOCALDNS_INSTALL,
    LINKUP_LOCALSERVER_PID_FILE,
};

pub fn stop() -> Result<(), CliError> {
    // Reset env vars back to what they were before
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

    shutdown()
}

pub fn shutdown() -> Result<(), CliError> {
    let local_stopped = stop_pid_file(
        &linkup_file_path(LINKUP_LOCALSERVER_PID_FILE),
        Signal::SIGINT,
    );

    let tunnel_stopped = stop_pid_file(&linkup_file_path(LINKUP_CLOUDFLARED_PID), Signal::SIGINT);

    if linkup_file_path(LINKUP_LOCALDNS_INSTALL).exists() {
        stop_localdns_services();
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

pub fn stop_pid_file(pid_file: &Path, signal: Signal) -> Result<(), CliError> {
    let stopped = match get_pid(pid_file) {
        Ok(pid) => match send_signal(&pid, signal) {
            Ok(_) => Ok(()),
            Err(PidError::NoSuchProcess(_)) => Ok(()),
            Err(e) => Err(CliError::StopErr(format!(
                "Could not send {} to {} pid {}: {}",
                signal,
                pid_file.display(),
                pid,
                e
            ))),
        },
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(CliError::StopErr(format!(
            "Could not get {} pid: {}",
            pid_file.display(),
            e
        ))),
    };

    if stopped.is_ok() {
        let _ = std::fs::remove_file(pid_file);
    }

    stopped
}

fn remove_service_env(directory: String, config_path: String) -> Result<(), CliError> {
    let config_dir = Path::new(&config_path).parent().ok_or_else(|| {
        CliError::SetServiceEnv(
            directory.clone(),
            "config_path does not have a parent directory".to_string(),
        )
    })?;

    let service_path = PathBuf::from(config_dir).join(&directory);

    let env_files_result = fs::read_dir(service_path);
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

        clear_env_file(&directory, &env_path)?;
    }

    Ok(())
}

fn stop_localdns_services() {
    let _ = services::caddy::stop();
    services::dnsmasq::stop();
}
