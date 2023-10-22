use crate::{
    extract_tracestate_session, first_subdomain, headers::HeaderName, random_animal,
    random_six_char, session_to_json, ConfigError, HeaderMap, NameKind, Session, SessionError,
    StringStore,
};

pub async fn get_request_session(
    string_store: &impl StringStore,
    url: &str,
    headers: &HeaderMap,
) -> Result<(String, Session), SessionError> {
    let url_name = first_subdomain(url);
    if let Some(config) = get_session_config(string_store, url_name.to_string()).await? {
        return Ok((url_name, config));
    }

    if let Some(forwarded_host) = headers.get(HeaderName::ForwardedHost) {
        let forwarded_host_name = first_subdomain(forwarded_host);
        if let Some(config) =
            get_session_config(string_store, forwarded_host_name.to_string()).await?
        {
            return Ok((forwarded_host_name, config));
        }
    }

    if let Some(referer) = headers.get("referer") {
        let referer_name = first_subdomain(referer);
        if let Some(config) = get_session_config(string_store, referer_name.to_string()).await? {
            return Ok((referer_name, config));
        }
    }

    if let Some(origin) = headers.get("origin") {
        let origin_name = first_subdomain(origin);
        if let Some(config) = get_session_config(string_store, origin_name.to_string()).await? {
            return Ok((origin_name, config));
        }
    }

    if let Some(tracestate) = headers.get("tracestate") {
        let trace_name = extract_tracestate_session(tracestate);
        if let Some(config) = get_session_config(string_store, trace_name.to_string()).await? {
            return Ok((trace_name, config));
        }
    }

    Err(SessionError::NoSuchSession(url.to_string()))
}

async fn get_session_config(
    string_store: &impl StringStore,
    name: String,
) -> Result<Option<Session>, SessionError> {
    let value = match string_store.get(name).await {
        Ok(Some(v)) => v,
        Ok(None) => return Ok(None),
        Err(e) => return Err(e),
    };

    let config_value: serde_json::Value =
        serde_json::from_str(&value).map_err(|e| SessionError::ConfigErr(e.to_string()))?;

    let session_config = config_value
        .try_into()
        .map_err(|e: ConfigError| SessionError::ConfigErr(e.to_string()))?;

    Ok(Some(session_config))
}

pub async fn store_session(
    string_store: &impl StringStore,
    config: Session,
    name_kind: NameKind,
    desired_name: String,
) -> Result<String, SessionError> {
    let name = choose_name(
        string_store,
        desired_name,
        config.session_token.clone(),
        name_kind,
    )
    .await?;

    let config_str = session_to_json(config);

    string_store.put(name.clone(), config_str).await?;

    Ok(name)
}

async fn choose_name(
    string_store: &impl StringStore,
    desired_name: String,
    session_token: String,
    name_kind: NameKind,
) -> Result<String, SessionError> {
    if desired_name.is_empty() {
        return new_session_name(string_store, name_kind, desired_name).await;
    }

    if let Some(session) = get_session_config(string_store, desired_name.clone()).await? {
        if session.session_token == session_token {
            return Ok(desired_name);
        }
    }

    new_session_name(string_store, name_kind, desired_name).await
}

async fn new_session_name(
    string_store: &impl StringStore,
    name_kind: NameKind,
    desired_name: String,
) -> Result<String, SessionError> {
    let mut key = String::new();

    if !desired_name.is_empty() && !string_store.exists(desired_name.clone()).await? {
        key = desired_name;
    }

    if key.is_empty() {
        let mut tried_animal_key = false;
        loop {
            let generated_key = if !tried_animal_key && name_kind == NameKind::Animal {
                tried_animal_key = true;
                generate_unique_animal_key(string_store, 20).await?
            } else {
                random_six_char()
            };

            if !string_store.exists(generated_key.clone()).await? {
                key = generated_key;
                break;
            }
        }
    }

    Ok(key)
}

async fn generate_unique_animal_key(
    string_store: &impl StringStore,
    max_attempts: usize,
) -> Result<String, SessionError> {
    for _ in 0..max_attempts {
        let generated_key = random_animal();
        if !string_store.exists(generated_key.clone()).await? {
            return Ok(generated_key);
        }
    }
    // Fallback to SixChar logic
    Ok(random_six_char())
}
