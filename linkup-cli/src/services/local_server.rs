use std::{
    env,
    fs::File,
    os::unix::process::CommandExt,
    process::{self, Stdio},
    time::Duration,
};

use anyhow::Context;
use linkup::{Session, TunneledSessionResponse, UpsertSessionRequest};
use reqwest::StatusCode;
use sysinfo::Pid;
use tokio::time::sleep;
use url::Url;

use linkup_clients::LocalServerClient;

use super::{PidError, ServiceId};
use crate::{Result, linkup_certs_dir_path, linkup_file_path, services, state::State};

const ID: ServiceId = ServiceId("linkup-local-server");
const NAME: &str = "Linkup local server";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Failed while handing file: {0}")]
    FileHandling(#[from] std::io::Error),
    #[error("Failed to stop pid: {0}")]
    StoppingPid(#[from] PidError),
    #[error("Failed to reach the local server")]
    ServerUnreachable,
}

pub fn url() -> Url {
    Url::parse("http://localhost:80").expect("linkup url invalid")
}

pub async fn start(state: &mut State) -> Result<()> {
    if super::find_pid(ID).is_some() {
        log::info!("Already running.");

        return Ok(());
    }

    log::info!("Starting...");
    spawn_process()?;

    let mut reachable = is_reachable().await;
    let mut attempts: u8 = 0;
    loop {
        match (reachable, attempts) {
            (true, _) => break,
            (false, 0..10) => {
                sleep(Duration::from_millis(1000)).await;
                attempts += 1;

                log::info!("Waiting for server... retry #{attempts}");

                reachable = is_reachable().await;
            }
            (false, 10..) => {
                log::error!("Failed to reach server");

                return Err(Error::ServerUnreachable.into());
            }
        }
    }
    log::info!("Ready!");

    log::info!("Uploading state...");
    match update_state(state).await {
        Ok(_) => {
            log::info!("Finished setting up!");
        }
        Err(e) => {
            log::error!("Failed to upload state: {e}");
            return Err(e);
        }
    }

    Ok(())
}

pub fn stop() {
    super::stop(ID);
}

pub fn find_pid() -> Option<Pid> {
    super::find_pid(ID)
}

async fn is_reachable() -> bool {
    matches!(
        LocalServerClient::new(&url()).health_check().await,
        Ok(true)
    )
}

async fn update_state(state: &mut State) -> Result<()> {
    let tunneled_session = upload_state(state).await?;

    state.linkup.session_name = tunneled_session.session_name;
    state
        .save()
        .expect("failed to update local state file with session name");

    Ok(())
}

pub async fn upload_state(state: &State) -> Result<TunneledSessionResponse> {
    let local_server_client = LocalServerClient::new(&services::local_server::url());

    let desired_session_name = &state.linkup.session_name;
    let session: Session = state.into();

    let upsert_request = UpsertSessionRequest::Named {
        session_token: session.session_token,
        desired_name: desired_session_name.to_string(),
        services: session.services,
        domains: session.domains,
        cache_routes: session.cache_routes,
    };

    let session_response = local_server_client.tunneled_session(&upsert_request).await;

    let session_response = match session_response {
        Ok(session_response) => session_response,
        Err(linkup_clients::LocalServerClientError::Response(StatusCode::CONFLICT, _)) => {
            // TODO(@augustoccesar)[2026-04-21]: Handle
            todo!("Create with a new name")
        }
        Err(error) => return Err(error.into()),
    };

    Ok(session_response)
}

fn spawn_process() -> Result<()> {
    log::debug!("Starting {}", NAME);

    let stdout_file = File::create(linkup_file_path("localserver-stdout"))?;
    let stderr_file = File::create(linkup_file_path("localserver-stderr"))?;

    let mut command =
        process::Command::new(env::current_exe().context("Failed to get the current executable")?);
    command.env(
        "RUST_LOG",
        "info,hickory_server=warn,hyper_util=warn,h2=warn,tower_http=info",
    );
    command.env("LINKUP_SERVICE_ID", ID.to_string());
    command.args([
        "server",
        "--certs-dir",
        linkup_certs_dir_path().to_str().unwrap(),
    ]);

    command
        .process_group(0)
        .stdout(stdout_file)
        .stderr(stderr_file)
        .stdin(Stdio::null())
        .spawn()?;

    Ok(())
}
