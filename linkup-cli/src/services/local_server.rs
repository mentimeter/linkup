use std::{
    env,
    fs::File,
    os::unix::process::CommandExt,
    process::{self, Stdio},
    time::Duration,
};

use anyhow::Context;
use reqwest::StatusCode;
use sysinfo::Pid;
use tokio::time::sleep;
use url::Url;

use linkup::{
    NameKind, Session, SessionKind, TunnelData, TunneledSessionResponse, UpsertSessionRequest,
    random_six_char,
};
use linkup_clients::{LocalServerClient, LocalServerClientError};

use super::{PidError, ServiceId};
use crate::{
    Result, linkup_certs_dir_path, linkup_file_path,
    services::log_tailer::{self, LogTailer},
    state::State,
};

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

pub async fn start() -> Result<LogTailer> {
    let freshly_spawned = super::find_pid(ID).is_none();
    if !freshly_spawned {
        log::info!("Already running.");
    } else {
        spawn_process()?;
    }

    // If the server was already running, don't "relog" the logs from the file.
    let tailer = log_tailer::tail_logs(linkup_file_path("localserver-stderr"), freshly_spawned);

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

    Ok(tailer)
}

pub fn stop() {
    super::stop(ID);
}

pub fn find_pid() -> Option<Pid> {
    super::find_pid(ID)
}

pub async fn is_reachable() -> bool {
    matches!(
        LocalServerClient::new(&url()).health_check().await,
        Ok(true)
    )
}

pub async fn update_state(state: &mut State) -> Result<TunnelData> {
    log::info!("Uploading state to server...");
    let tunneled_session = upload_tunneled_state(state).await?;

    log::info!("Updating local state file...");
    state.linkup.session_name = tunneled_session.session_name;
    state.linkup.kind = SessionKind::Tunneled;
    state
        .save()
        .expect("failed to update local state file with session name");

    Ok(tunneled_session.tunnel_data)
}

pub async fn update_isolated_state(state: &mut State) -> Result<()> {
    let session_name = if !state.linkup.session_name.is_empty() {
        state.linkup.session_name.clone()
    } else {
        random_six_char()
    };

    let client = LocalServerClient::new(&url());
    let upsert_request = build_named_upsert_request(&session_name, state);
    client.isolated_session(&upsert_request).await?;

    state.linkup.session_name = session_name;
    state.linkup.kind = SessionKind::Isolated;

    Ok(())
}

fn build_named_upsert_request(session_name: &str, state: &State) -> UpsertSessionRequest {
    let session: Session = state.into();

    UpsertSessionRequest::Named {
        desired_name: session_name.to_string(),
        session_token: session.session_token,
        services: session.services,
        domains: session.domains,
        cache_routes: session.cache_routes,
    }
}

async fn upload_tunneled_state(state: &State) -> Result<TunneledSessionResponse> {
    let local_server_client = LocalServerClient::new(&url());

    let session: Session = state.into();

    let desired_session_name =
        (!state.linkup.session_name.is_empty()).then(|| state.linkup.session_name.clone());

    let upsert_request = match desired_session_name {
        Some(desired_name) => build_named_upsert_request(&desired_name, state),
        None => {
            let session_token =
                (!session.session_token.is_empty()).then_some(session.session_token);

            UpsertSessionRequest::Unnamed {
                name_kind: NameKind::Animal,
                session_token,
                services: session.services.clone(),
                domains: session.domains.clone(),
                cache_routes: session.cache_routes.clone(),
            }
        }
    };

    let session_response = local_server_client.tunneled_session(&upsert_request).await;

    let session_response = match session_response {
        Ok(session_response) => session_response,
        Err(LocalServerClientError::Response(StatusCode::CONFLICT, _)) => {
            log::debug!(
                "Requested name from state file already exists, attempting to create with a new name"
            );

            let unnamed_request = UpsertSessionRequest::Unnamed {
                name_kind: NameKind::Animal,
                session_token: None,
                services: session.services,
                domains: session.domains,
                cache_routes: session.cache_routes,
            };

            local_server_client
                .tunneled_session(&unnamed_request)
                .await?
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

    let user_filter = env::var("LINKUP_LOG").unwrap_or_else(|_| "info".to_string());
    let linkup_log =
        format!("{user_filter},hickory_server=warn,hyper_util=warn,h2=warn,tower_http=info");

    command.env("LINKUP_LOG", linkup_log);
    command.env("RUST_LOG_STYLE", "always");
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
