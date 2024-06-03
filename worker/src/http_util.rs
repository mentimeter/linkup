use linkup::{unpack_cookie_header, HeaderMap as LinkupHeaderMap};
// use reqwest::{Method as ReqwestMethod, Response as ReqwestResponse};
use std::convert::TryFrom;
use worker::{
    console_log, Headers as CfHeaders, Method as CfMethod, Response as CfResponse,
    Result as CfResult,
};

const SET_COOKIE: &str = "set-cookie";

pub fn plaintext_error(msg: impl Into<String>, status: u16) -> CfResult<CfResponse> {
    let mut resp = CfResponse::error(msg, status)?;
    let headers = resp.headers_mut();
    headers.set("Content-Type", "text/plain")?;

    Ok(resp)
}

// pub fn convert_cf_method_to_reqwest(
//     cf_method: &CfMethod,
// ) -> Result<ReqwestMethod, http::method::InvalidMethod> {
//     let method_str = match cf_method {
//         CfMethod::Get => "GET",
//         CfMethod::Post => "POST",
//         CfMethod::Put => "PUT",
//         CfMethod::Delete => "DELETE",
//         CfMethod::Options => "OPTIONS",
//         CfMethod::Head => "HEAD",
//         CfMethod::Patch => "PATCH",
//         CfMethod::Connect => "CONNECT",
//         CfMethod::Trace => "TRACE",
//     };

//     ReqwestMethod::try_from(method_str)
// }

// pub async fn convert_reqwest_response_to_cf(
//     response: ReqwestResponse,
//     extra_headers: &LinkupHeaderMap,
// ) -> worker::Result<CfResponse> {
//     let status = response.status();
//     let headers = response.headers().clone();

//     let body_bytes = match response.bytes().await {
//         Ok(bytes) => bytes,
//         Err(_) => return CfResponse::error("Error reading response body", 502),
//     };

//     let cf_response = match CfResponse::from_bytes(body_bytes.to_vec()) {
//         Ok(response) => response,
//         Err(_) => return CfResponse::error("Error creating response body", 500),
//     };

//     let mut cf_headers = CfHeaders::from(headers);

//     for (key, value) in extra_headers.into_iter() {
//         let header_res = cf_headers.set(&key, &value);
//         if header_res.is_err() {
//             console_log!("failed to set response header: {}", header_res.unwrap_err());
//         }
//     }

//     // WASM / web_sys does not support getSetCookie, so we unwrap cookies ourselves
//     if let Ok(Some(folded_cookies)) = cf_headers.get(SET_COOKIE) {
//         let cookies = unpack_cookie_header(folded_cookies);
//         cf_headers.delete(SET_COOKIE).expect("set-cookie-valid");

//         for cookie in cookies {
//             cf_headers
//                 .append(SET_COOKIE, &cookie.to_string())
//                 .expect("set-cookie-valid");
//         }
//     }

//     let cf_response = cf_response.with_headers(cf_headers);
//     let cf_response = cf_response.with_status(status.into());

//     Ok(cf_response)
// }
