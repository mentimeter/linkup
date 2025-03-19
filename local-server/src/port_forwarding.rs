// To avoid needing to sudo every time we start or stop the local server, we add the port
// forwardings 8080->80 and 8443->443 so that we can start the local server on 8080 and 8443
// while still being able to resolve the dnsmasq to 80 and 443.
// This will make so that the user will need to sudo once only and then after the port forwardings
// are in place it won't be necessary anymore.
//
// Note that, since we are using `pfctl -f <PORTS CONF>`, it will be reset if the user restart their
// computer. For that reason, we are also storing the flag file on a tmp folder, so that the check
// also reset on a computer restart.
//
// This means that if the user restart the computer, the first run of linkup will require sudo
// again.

use std::{io::Write, path::Path, process};

const PORTS_CONFIG: &str = "/etc/pf.linkup.ports.conf";
const FLAG_FILE: &str = "/tmp/port_forwarding_active";

pub fn is_port_forwarding_active() -> bool {
    Path::new(FLAG_FILE).exists()
}

pub fn setup_port_forwarding() -> Result<(), Box<dyn std::error::Error>> {
    tracing::event!(tracing::Level::DEBUG, "Setting up port forwarding.");

    let content = r#"rdr pass on lo0 inet proto tcp from any to any port 80 -> 127.0.0.1 port 8080
rdr pass on lo0 inet proto tcp from any to any port 443 -> 127.0.0.1 port 8443
"#;

    let mut tee_cmd = process::Command::new("sudo")
        .args(["tee", PORTS_CONFIG])
        .stdin(process::Stdio::piped()) // We will write to here to persist the file
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .spawn()?;

    if let Some(mut stdin) = tee_cmd.stdin.take() {
        stdin.write_all(content.as_bytes())?;
    }

    let status = tee_cmd.wait()?;
    if !status.success() {
        tracing::event!(tracing::Level::ERROR, "Failed to write {PORTS_CONFIG}");

        return Err("Failed to write port forwarding config".into());
    } else {
        tracing::event!(
            tracing::Level::DEBUG,
            "Written port forwardings into {PORTS_CONFIG}",
        );
    }

    let enable_output = process::Command::new("sudo")
        .args(["pfctl", "-e"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::piped()) // We will read from here to check if is already running
        .output()?;
    if !enable_output.status.success() {
        let enable_stderr = String::from_utf8(enable_output.stderr).unwrap();
        if !enable_stderr.contains("pf already enabled") {
            tracing::event!(tracing::Level::ERROR, "Failed to enable port forwarding");

            return Err("Failed to enable port forwarding".into());
        } else {
            tracing::event!(tracing::Level::DEBUG, "Port forwarding already enabled.");
        }
    }

    let status = process::Command::new("sudo")
        .args(["pfctl", "-f", PORTS_CONFIG])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()?;
    if !status.success() {
        tracing::event!(
            tracing::Level::ERROR,
            "Failed to load {PORTS_CONFIG} into pfctl."
        );

        return Err("Failed to load port forwarding rules".into());
    } else {
        tracing::event!(tracing::Level::DEBUG, "Loaded {PORTS_CONFIG} into pfctl.");
    }

    std::fs::write(Path::new(FLAG_FILE), "")?;

    Ok(())
}
