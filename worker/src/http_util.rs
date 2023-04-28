use reqwest::{Method as ReqwestMethod, Response as ReqwestResponse};
use std::{collections::HashMap, convert::TryFrom};
use worker::{console_log, Headers as CfHeaders, Method as CfMethod, Response as CfResponse};

pub fn convert_cf_method_to_reqwest(
    cf_method: &CfMethod,
) -> Result<ReqwestMethod, http::method::InvalidMethod> {
    let method_str = match cf_method {
        CfMethod::Get => "GET",
        CfMethod::Post => "POST",
        CfMethod::Put => "PUT",
        CfMethod::Delete => "DELETE",
        CfMethod::Options => "OPTIONS",
        CfMethod::Head => "HEAD",
        CfMethod::Patch => "PATCH",
        CfMethod::Connect => "CONNECT",
        CfMethod::Trace => "TRACE",
    };

    ReqwestMethod::try_from(method_str)
}

pub fn merge_headers(
    original_headers: HashMap<String, String>,
    extra_headers: HashMap<String, String>,
) -> reqwest::header::HeaderMap {
    let mut header_map = reqwest::header::HeaderMap::new();
    for (key, value) in original_headers
        .into_iter()
        .chain(extra_headers.into_iter())
    {
        if let Ok(header_name) = reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
            if let Ok(header_value) = reqwest::header::HeaderValue::from_str(&value) {
                header_map.append(header_name, header_value);
            }
        }
    }
    header_map
}

pub async fn convert_reqwest_response_to_cf(
    response: ReqwestResponse,
) -> worker::Result<CfResponse> {
    let status = response.status();
    let headers = response.headers().clone();

    let body_bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(_) => return CfResponse::error("Error reading response body", 502),
    };

    let cf_response = match CfResponse::from_bytes(body_bytes.to_vec()) {
        Ok(response) => response,
        Err(_) => return CfResponse::error("Error creating response body", 500),
    };
    let cf_headers = CfHeaders::from(headers);
    let cf_response = cf_response.with_headers(cf_headers.clone());
    let cf_response = cf_response.with_status(status.into());

    Ok(cf_response)
}
