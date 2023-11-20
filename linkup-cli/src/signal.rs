use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::fs::{self, File};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PidError {
    #[error("no pid file: {0}")]
    NoPidFile(String),
    #[error("bad pid file: {0}")]
    BadPidFile(String),
    #[error("signal error: {0}")]
    SignalErr(String),
    #[error("no such process: {0}")]
    NoSuchProcess(String),
}

pub fn send_signal(pid_str: &str, signal: Signal) -> Result<(), PidError> {
    // Parse the PID string to a i32
    let pid_num = i32::from_str(pid_str).map_err(|e| PidError::BadPidFile(e.to_string()))?;

    // Create a Pid from the i32
    let pid = Pid::from_raw(pid_num);

    match kill(pid, Some(signal)) {
        Ok(_) => Ok(()),
        Err(nix::Error::ESRCH) => Err(PidError::NoSuchProcess(pid_str.to_string())),
        Err(e) => Err(PidError::SignalErr(e.to_string())),
    }
}

pub fn get_pid(file_path: &Path) -> Result<String, PidError> {
    if let Err(e) = File::open(file_path) {
        return Err(PidError::NoPidFile(e.to_string()));
    }

    match fs::read_to_string(file_path) {
        Ok(content) => Ok(content.trim().to_string()),
        Err(e) => Err(PidError::BadPidFile(e.to_string())),
    }
}
