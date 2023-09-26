use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use crate::{linkup_dir_path, linkup_file_path, stop::kill_pid_file, CliError, Result};

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
        .map_err(|err| CliError::StartCaddy(err.to_string()))?;

    Ok(())
}

pub fn stop() -> Result<()> {
    let dnsmasq_stopped = kill_pid_file(PID_FILE);
    if dnsmasq_stopped.is_ok() {
        let _ = std::fs::remove_file(linkup_file_path(PID_FILE));
    }

    Ok(())
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
