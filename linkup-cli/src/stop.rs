use std::fs::{self, File};

use crate::signal::{send_sigint, PidError};
use crate::{linkup_file_path, CliError, LINKUP_CLOUDFLARED_PID, LINKUP_LOCALSERVER_PID_FILE};

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
