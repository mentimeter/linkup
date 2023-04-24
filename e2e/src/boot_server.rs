use anyhow::Result;
use std::process::{Child, Command, Stdio};

pub fn boot_background_web_server(port: u16, name: String) -> Result<Child> {
    let command = format!(
        "while true; do echo 'HTTP/1.1 200 OK\r\n\r\n{}' | nc -l {}; done",
        name, port
    );

    let c = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(c)
}
