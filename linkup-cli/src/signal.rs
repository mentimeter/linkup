use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
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
}

pub fn send_sigint(pid_str: &str) -> Result<(), PidError> {
    // Parse the PID string to a i32
    let pid_num = i32::from_str(pid_str).map_err(|e| PidError::BadPidFile(e.to_string()))?;

    // Create a Pid from the i32
    let pid = Pid::from_raw(pid_num);

    match kill(pid, Some(Signal::SIGINT)) {
        Ok(_) => Ok(()),
        Err(e) => Err(PidError::SignalErr(e.to_string())),
    }
}
