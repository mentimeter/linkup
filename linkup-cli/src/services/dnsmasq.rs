use std::{fs, process::Command};

use nix::sys::signal::Signal;
use std::fmt::Write;

use crate::{linkup_dir_path, linkup_file_path, stop::stop_pid_file, CliError, Result};

const PORT: u16 = 8053;
const CONF_FILE: &str = "dnsmasq-conf";
const LOG_FILE: &str = "dnsmasq-log";
const PID_FILE: &str = "dnsmasq-pid";

pub fn start(domains: Vec<String>, session_name: String) -> Result<()> {
    let conf_file_path = write_dnsmaq_conf(domains, session_name)?;

    Command::new("dnsmasq")
        .current_dir(linkup_dir_path())
        .arg("--log-queries")
        .arg("-C")
        .arg(conf_file_path)
        // .stdout(Stdio::null())
        // .stderr(Stdio::null())
        .status()
        .map_err(|err| CliError::StartDNSMasq(err.to_string()))?;

    Ok(())
}

// TODO(augustoccesar)[2023-09-26]: Do we really want to swallow these errors?
pub fn stop() {
    let _ = stop_pid_file(&linkup_file_path(PID_FILE), Signal::SIGTERM);
}

fn write_dnsmaq_conf(domains: Vec<String>, session_name: String) -> Result<String> {
    let conf_file_path = linkup_file_path(CONF_FILE);
    let logfile_path = linkup_file_path(LOG_FILE);
    let pidfile_path = linkup_file_path(PID_FILE);
    let local_domains_template = domains.iter().fold(String::new(), |mut acc, d| {
        let _ = write!(
            acc,
            "address=/{}.{}/127.0.0.1\naddress=/{}.{}/::1\nlocal=/{}.{}/\n",
            session_name, d, session_name, d, session_name, d
        );
        acc
    });

    let dnsmasq_template = format!(
        "{}

port={}
log-facility={}
pid-file={}\n",
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
