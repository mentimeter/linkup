use std::{
    env,
    fs::File,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Stdio},
};

use anyhow::Context;

use crate::{commands::local_dns, linkup_file_path, local_config::LocalState, Result};

use super::BackgroundService;

pub struct LocalDnsServer {
    stdout_file_path: PathBuf,
    stderr_file_path: PathBuf,
}

impl LocalDnsServer {
    pub fn new() -> Self {
        Self {
            stdout_file_path: linkup_file_path("localdns-stdout"),
            stderr_file_path: linkup_file_path("localdns-stderr"),
        }
    }

    fn start(&self, session_name: &str, domains: &[String]) -> Result<()> {
        log::debug!("Starting {}", Self::NAME);

        let stdout_file = File::create(&self.stdout_file_path)?;
        let stderr_file = File::create(&self.stderr_file_path)?;

        let mut command = process::Command::new(
            env::current_exe().context("Failed to get the current executable")?,
        );
        command.env("RUST_LOG", "debug");
        command.env("LINKUP_SERVICE_ID", Self::ID);
        command.args([
            "server",
            "dns",
            "--session-name",
            session_name,
            "--domains",
            &domains.join(","),
        ]);

        command
            .process_group(0)
            .stdout(stdout_file)
            .stderr(stderr_file)
            .stdin(Stdio::null())
            .spawn()?;

        Ok(())
    }
}

impl BackgroundService for LocalDnsServer {
    const ID: &str = "linkup-local-dns-server";
    const NAME: &str = "Local DNS server";

    async fn run_with_progress(
        &self,
        state: &mut LocalState,
        status_sender: std::sync::mpsc::Sender<super::RunUpdate>,
    ) -> Result<()> {
        self.notify_update(&status_sender, super::RunStatus::Starting);

        let session_name = state.linkup.session_name.clone();
        let domains = state.domain_strings();

        if !local_dns::is_installed(&domains) {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Skipped,
                "Not installed",
            );

            return Ok(());
        }

        if let Err(e) = self.start(&session_name, &domains) {
            self.notify_update_with_details(
                &status_sender,
                super::RunStatus::Error,
                "Failed to start",
            );

            return Err(e);
        }

        self.notify_update(&status_sender, super::RunStatus::Started);

        Ok(())
    }
}
