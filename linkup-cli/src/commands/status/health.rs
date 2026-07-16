use std::time::Duration;

use colored::{ColoredString, Colorize};
use linkup::HeaderMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub enum ServerStatus {
    Ok,
    Error,
    Timeout,
    Loading,
}

impl ServerStatus {
    pub fn colored(&self) -> ColoredString {
        match self {
            ServerStatus::Ok => "ok".blue(),
            ServerStatus::Error => "error".yellow(),
            ServerStatus::Timeout => "timeout".yellow(),
            ServerStatus::Loading => "loading".normal(),
        }
    }
}

pub fn server_status(
    url: &str,
    acceptable_statuses_override: Option<&Vec<u16>>,
    extra_headers: Option<HeaderMap>,
) -> ServerStatus {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build();

    match client {
        Ok(client) => {
            let mut request = client.get(url);

            if let Some(extra_headers) = extra_headers {
                request = request.headers(extra_headers.into());
            }

            match request.send() {
                Ok(response) => {
                    log::debug!(
                        "'{}' responded with status: {}. Acceptable statuses: {:?}",
                        url,
                        response.status().as_u16(),
                        acceptable_statuses_override
                    );

                    match (acceptable_statuses_override, response.status()) {
                        (None, status) => {
                            if !status.is_server_error() {
                                ServerStatus::Ok
                            } else {
                                ServerStatus::Error
                            }
                        }
                        (Some(override_statuses), status) => {
                            if override_statuses.contains(&status.as_u16()) {
                                ServerStatus::Ok
                            } else {
                                ServerStatus::Error
                            }
                        }
                    }
                }
                Err(_) => ServerStatus::Error,
            }
        }
        Err(_) => ServerStatus::Error,
    }
}
