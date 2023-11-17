use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use nix::sys::signal::Signal;

use crate::stop::stop_pid_file;
use crate::{linkup_dir_path, linkup_file_path, CliError, Result};

const PORT: u16 = 8053;
const CONF_FILE: &str = "dnsmasq-conf";
const LOG_FILE: &str = "dnsmasq-log";
const PID_FILE: &str = "dnsmasq-pid";

pub fn start() -> Result<()> {
    let conf_file_path = linkup_file_path(CONF_FILE);
    let logfile_path = linkup_file_path(LOG_FILE);
    let pidfile_path = linkup_file_path(PID_FILE);

    if fs::write(&logfile_path, "").is_err() {
        return Err(CliError::WriteFile(format!(
            "Failed to write dnsmasq log file at {}",
            logfile_path.display()
        )));
    }

    write_conf_file(&conf_file_path, &logfile_path, &pidfile_path)?;

    Command::new("dnsmasq")
        .current_dir(linkup_dir_path())
        .arg("--log-queries")
        .arg("-C")
        .arg(conf_file_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| CliError::StartDNSMasq(err.to_string()))?;

    Ok(())
}

// TODO(augustoccesar)[2023-09-26]: Do we really want to swallow these errors?
pub fn stop() {
    let _ = stop_pid_file(&linkup_file_path(PID_FILE), Signal::SIGTERM);
}

fn write_conf_file(conf_file_path: &Path, logfile_path: &Path, pidfile_path: &Path) -> Result<()> {
    let dnsmasq_template = format!(
        "
            address=/#/127.0.0.1
            port={}
            log-facility={}
            pid-file={}
        ",
        PORT,
        logfile_path.display(),
        pidfile_path.display(),
    );

    if fs::write(conf_file_path, dnsmasq_template).is_err() {
        return Err(CliError::WriteFile(format!(
            "Failed to write dnsmasq config at {}",
            conf_file_path.display()
        )));
    }

    Ok(())
}
