use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::fs::{self, File};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

pub use nix::sys::signal::Signal;

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

// Get the pid from a pidfile, but only return Some in case the pidfile is valid and the written pid on the file
// is running.
pub fn get_running_pid(file_path: &Path) -> Option<String> {
    let pid = match get_pid(file_path) {
        Ok(pid) => pid,
        Err(_) => return None,
    };

    let pid = match u32::from_str(&pid) {
        Ok(pid) => pid,
        Err(_) => return None, // TODO: Do we want to be loud about this?
    };

    let system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::everything()),
    );

    match system.process(sysinfo::Pid::from_u32(pid)) {
        Some(_) => return Some(pid.to_string()),
        None => None,
    }
}

pub fn stop_pid_file(pid_file: &Path, signal: Signal) -> Result<(), PidError> {
    let stopped = match get_pid(pid_file) {
        Ok(pid) => match send_signal(&pid, signal) {
            Ok(_) => Ok(()),
            Err(PidError::NoSuchProcess(_)) => Ok(()),
            Err(e) => Err(e),
        },
        Err(PidError::NoPidFile(_)) => Ok(()),
        Err(e) => Err(e),
    };

    if stopped.is_ok() {
        let _ = std::fs::remove_file(pid_file);
    }

    stopped
}
