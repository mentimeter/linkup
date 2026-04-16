pub mod local_session;
pub mod preview_session;
pub mod tunnel;

use axum::response::IntoResponse;
use http::StatusCode;
use linkup::{ConfigError, Domain, NameKind, Session, SessionAllocator, SessionService};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::{http_error::HttpError, kv_store::CfWorkerStringStore, worker_state::WorkerState};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum UpsertSessionRequest {
    Named {
        desired_name: String,
        session_token: String,
        services: Vec<SessionService>,
        domains: Vec<Domain>,
        #[serde(
            default,
            serialize_with = "linkup::serde_ext::serialize_opt_vec_regex",
            deserialize_with = "linkup::serde_ext::deserialize_opt_vec_regex"
        )]
        cache_routes: Option<Vec<Regex>>,
    },
    Unnamed {
        services: Vec<SessionService>,
        domains: Vec<Domain>,
        #[serde(
            default,
            serialize_with = "linkup::serde_ext::serialize_opt_vec_regex",
            deserialize_with = "linkup::serde_ext::deserialize_opt_vec_regex"
        )]
        cache_routes: Option<Vec<Regex>>,
    },
}

// TODO(augustoccesar)[2026-04-13]: This methods now exists because both the endpoints to
//  create a preview session and a local session are exactly the same with the only
//  difference being on the name generator kind.
//  We should probably deprecate them as separate endpoints and create a new one that
//  can take the name generator as part of the request.
async fn handle_session_upsert(
    state: WorkerState,
    req: UpsertSessionRequest,
    name_kind: NameKind,
) -> impl IntoResponse {
    let store = CfWorkerStringStore::new(state.sessions_kv.clone());
    let sessions = SessionAllocator::new(&store);

    let desired_name = match &req {
        UpsertSessionRequest::Named { desired_name, .. } => desired_name.clone(),
        UpsertSessionRequest::Unnamed { .. } => String::new(),
    };

    let session: Session = match req.try_into() {
        Ok(conf) => conf,
        Err(e) => {
            return HttpError::new(
                format!("Failed to parse server config: {} - Worker", e),
                StatusCode::BAD_REQUEST,
            )
            .into_response();
        }
    };

    let session_name = sessions
        .store_session(session, name_kind, &desired_name)
        .await;

    let name = match session_name {
        Ok(session_name) => session_name,
        Err(e) => {
            return HttpError::new(
                format!("Failed to store server config: {}", e),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .into_response();
        }
    };

    (StatusCode::OK, name).into_response()
}

impl TryFrom<UpsertSessionRequest> for Session {
    type Error = ConfigError;

    fn try_from(req: UpsertSessionRequest) -> Result<Self, Self::Error> {
        let (session_token, services, domains, cache_routes) = match req {
            UpsertSessionRequest::Named {
                services,
                domains,
                cache_routes,
                session_token,
                ..
            } => (session_token, services, domains, cache_routes),
            UpsertSessionRequest::Unnamed {
                services,
                domains,
                cache_routes,
            } => (
                "preview_session".to_string(),
                services,
                domains,
                cache_routes,
            ),
        };

        let session = Self {
            session_token,
            services,
            domains,
            cache_routes,
        };

        validate_not_empty(&session)?;
        validate_services(&session)?;

        Ok(session)
    }
}

fn validate_not_empty(session: &Session) -> Result<(), ConfigError> {
    if session.services.is_empty() {
        return Err(ConfigError::Empty);
    }

    if session.domains.is_empty() {
        return Err(ConfigError::Empty);
    }

    Ok(())
}

fn validate_services(session: &Session) -> Result<(), ConfigError> {
    let mut service_names: HashSet<&str> = HashSet::new();

    for service in &session.services {
        service_names.insert(&service.name);
    }

    for domain in &session.domains {
        if !service_names.contains(&domain.default_service.as_str()) {
            return Err(ConfigError::NoSuchService(
                domain.default_service.to_string(),
            ));
        }

        if let Some(routes) = &domain.routes {
            for route in routes {
                if !service_names.contains(&route.service.as_str()) {
                    return Err(ConfigError::NoSuchService(route.service.to_string()));
                }
            }
        }
    }

    Ok(())
}
