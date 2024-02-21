use std::{
    fs,
    process::{Command, Stdio},
};

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::fmt::Write;

use crate::{linkup_dir_path, linkup_file_path, stop::stop_pid_file, CliError, Result};

const PORT: u16 = 8053;
const CONF_FILE: &str = "dnsmasq-conf";
const LOG_FILE: &str = "dnsmasq-log";
const PID_FILE: &str = "dnsmasq-pid";

pub fn start() -> Result<()> {
    let conf_file_path = write_dnsmaq_conf(None)?;

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

pub fn write_dnsmaq_conf(local_domains: Option<Vec<String>>) -> Result<String> {
    let conf_file_path = linkup_file_path(CONF_FILE);
    let logfile_path = linkup_file_path(LOG_FILE);
    let pidfile_path = linkup_file_path(PID_FILE);
    let mut local_domains_template = String::new();
    if let Some(local_domains) = local_domains {
        local_domains_template = local_domains.iter().fold(String::new(), |mut acc, d| {
            let _ = write!(acc, "address=/{}/127.0.0.1\naddress=/{}/::1\n", d, d);
            acc
        });
    }

    let dnsmasq_template = format!(
        "
# Set of domains that should be routed locally
{}

# Other dnsmasq config options
server=1.1.1.1
port={}
log-facility={}
pid-file={}
        ",
        local_domains_template,
        PORT,
        logfile_path.display(),
        pidfile_path.display(),
    );

    if fs::write(conf_file_path, dnsmasq_template).is_err() {
        return Err(CliError::WriteFile(format!(
            "Failed to write dnsmasq config at {}",
            linkup_file_path(CONF_FILE).display()
        )));
    }

    Ok(linkup_file_path(CONF_FILE)
        .to_str()
        .expect("const path known to be valid")
        .to_string())
}

pub fn reload_dnsmasq_conf() -> Result<()> {
    let pidfile_path = linkup_file_path(PID_FILE);
    let pid_str = fs::read_to_string(pidfile_path).map_err(|e| {
        CliError::RebootDNSMasq(format!(
            "Failed to read PID file at {}: {}",
            linkup_file_path(PID_FILE).display(),
            e
        ))
    })?;

    // Parse the PID from the file content
    let pid = pid_str
        .trim()
        .parse::<i32>()
        .map_err(|e| CliError::RebootDNSMasq(format!("Invalid PID value: {}", e)))?;

    // Send SIGHUP signal to the dnsmasq process
    kill(Pid::from_raw(pid), Signal::SIGHUP)
        .map_err(|e| CliError::RebootDNSMasq(format!("Failed to send SIGHUP to dnsmasq: {}", e)))?;

    Ok(())
}
