use std::collections::HashMap;
use rand::Rng;
use thiserror::Error;

mod server_config;
mod name_gen;
mod memory_session_store;

pub use server_config::*;
pub use name_gen::new_session_name;
pub use memory_session_store::*;
use url::Url;

pub trait SessionStore {
    fn get(&self, name: &String) -> Option<ServerConfig>;
    fn new(&self, config: ServerConfig, name_kind: NameKind, desired_name: Option<String>) -> String;
}

#[derive(PartialEq)]
pub enum NameKind {
    Animal,
    SixChar,
}


#[derive(Error, Debug)]
pub enum SessionError {
    #[error("no session found for request {0}")]
    NoSuchSession(String) // Add known headers to error
}

pub fn get_request_session<T: SessionStore>(url: String, headers: HashMap<String, String>, store: &T) -> Result<(String, ServerConfig), SessionError> {
    let url_name = first_subdomain(&url);
    if let Some(config) = store.get(&url_name) {
        return Ok((url_name, config));
    }

    if let Some(referer) = headers.get("referer") {
        let referer_name = first_subdomain(referer);
        if let Some(config) = store.get(&referer_name) {
            return Ok((referer_name, config));
        }
    }

    if let Some(tracestate) = headers.get("tracestate") {
        let trace_name = extract_tracestate_session(tracestate);
        if let Some(config) = store.get(&trace_name) {
            return Ok((trace_name, config));
        }
    }

    Err(SessionError::NoSuchSession(url))
}

pub fn get_additional_headers(url: String, headers: HashMap<String, String>, name: &String, service: &String) -> HashMap<String, String> {
    let mut additional_headers = HashMap::new();

    if !headers.contains_key("traceparent") {
        let mut rng = rand::thread_rng();
        let trace: [u8; 16] = rng.gen();
        let parent: [u8; 8] = rng.gen();
        let version: [u8; 1] = [0];
        let flags: [u8; 1] = [0];

        let trace_hex = hex::encode(trace);
        let parent_hex = hex::encode(parent);
        let version_hex = hex::encode(version);
        let flags_hex = hex::encode(flags);

        let traceparent = format!("{}-{}-{}-{}", version_hex, trace_hex, parent_hex, flags_hex);
        additional_headers.insert("traceparent".to_string(), traceparent);
    }

    let tracestate = headers.get("tracestate");
    let serpress_session = format!("serpress-session={}", name);
    let serpress_service = format!("serpress-service={}", service);
    match tracestate {
        Some(ts) if !ts.contains(&serpress_session) => {
            let new_tracestate = format!("{},{},{}", ts, serpress_session, serpress_service);
            additional_headers.insert("tracestate".to_string(), new_tracestate);
        }
        None => {
            let new_tracestate = format!("{},{}", serpress_session, serpress_service);
            additional_headers.insert("tracestate".to_string(), new_tracestate);
        }
        _ => {}
    }

    if !headers.contains_key("X-Forwarded-Host") {
        additional_headers.insert("X-Forwarded-Host".to_string(), get_target_domain(&url, name));
    }

    additional_headers
}

// Returns a url for the destination service and the service name, if the request could be served by the config
pub fn get_target_url(url: String, headers: HashMap<String, String>, config: &ServerConfig, session_name: &String) -> Option<(String, String)> {
    let target = Url::parse(&url).unwrap();
    let tracestate = headers.get("tracestate");
    let path = target.path();

    // If the request hit serpress before, we shouldn't need to do routing again.
    if let Some(tracestate_value) = tracestate {
        let service_name = extract_tracestate_service(tracestate_value);
        if !service_name.is_empty() {
            if let Some(service) = config.services.get(&service_name) {
                // We don't want to re-apply path_modifiers here, they should have been applied already
                let target = redirect(target, &service.origin, None);
                return Some((String::from(target), service_name))
            }
        }
    }

    let target_domain = get_target_domain(&url, &session_name);
    if let Some(domain) = config.domains.get(&target_domain) {
        let service_name = domain.routes.iter()
            .find_map(|route| {
                if route.path.is_match(path) {
                    Some(route.service.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| domain.default_service.clone());
        println!("target service: {:#?}", service_name);
        
        if let Some(service) = config.services.get(&service_name) {
            let mut new_path = path.to_string();
            for modifier in &service.path_modifiers {
                if modifier.source.is_match(&new_path) {
                    new_path = modifier.source.replace_all(&new_path, &modifier.target).to_string();
                }
            }


            let target = redirect(target, &service.origin, Some(new_path));
            return Some((String::from(target), service_name))
        }
    }

    None
}

fn redirect(mut target: Url, source: &Url, path: Option<String>) -> Url {
    target.set_host(source.host_str().clone()).unwrap();
    target.set_scheme(source.scheme().clone()).unwrap();

    if let Some(port) = source.port() {
        target.set_port(Some(port)).unwrap();
    }

    if let Some(p) = path {
        target.set_path(&p);
    }

    target
}

fn get_target_domain(url: &String, session_name: &String) -> String {
    let without_schema = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or(url);
    
    let domain_with_path : String = if first_subdomain(url) == *session_name {
        without_schema.strip_prefix(&format!("{}.", session_name)).map(String::from).unwrap_or_else(|| without_schema.to_string())
    } else {
        without_schema.to_string()
    };


    domain_with_path.split('/').collect::<Vec<_>>()[0].to_string()
}

fn first_subdomain(url: &String) -> String {
    let without_schema = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")).unwrap_or(url);
    let parts: Vec<&str> = without_schema.split('.').collect();
    if parts.len() <= 2 {
        String::from("")
    } else {
        String::from(parts[0])
    }
}

fn extract_tracestate_session(tracestate: &String) -> String {
    extrace_tracestate(tracestate, String::from("serpress-session"))
}

fn extract_tracestate_service(tracestate: &String) -> String {
    extrace_tracestate(tracestate, String::from("serpress-service"))
}

fn extrace_tracestate(tracestate: &String, serpress_key: String) -> String {
    tracestate
        .split(',')
        .filter_map(|kv| {
            let mut parts = kv.splitn(2, '=');
            let key = parts.next()?;
            let value = parts.next()?;
            if key.trim() == serpress_key {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .next()
        .unwrap_or_else(|| "".to_string())
}


#[cfg(test)]
mod tests {
    use super::*;


    const CONF_STR: &str = r#"
    services:
      - name: frontend
        location: http://localhost:8000
        path_modifiers:
          - source: /foo/(.*)
            target: /bar/$1
      - name: backend
        location: http://localhost:8001/
    domains:
      - domain: example.com
        default_service: frontend
        routes:
          - path: /api/v1/.*
            service: backend
      - domain: api.example.com
        default_service: backend
    "#;

    #[test]
    fn test_get_request_session_by_subdomain() {
        let session_store = MemorySessionStore::new();

        let config = new_server_config(String::from(CONF_STR)).unwrap();

        let name = session_store.new(config, NameKind::Animal, None);

        // Normal subdomain
        get_request_session(format!("{}.example.com", name), HashMap::new(), &session_store).unwrap();

        // Referer
        let mut referer_headers: HashMap<String, String> = HashMap::new();
        // TODO check header capitalization
        referer_headers.insert(format!("referer"), format!("http://{}.example.com", name));
        get_request_session(format!("example.com"), referer_headers, &session_store).unwrap();

        // Trace state
        let mut trace_headers: HashMap<String, String> = HashMap::new();
        trace_headers.insert(format!("tracestate"), format!("some-other=xyz,serpress-session={}", name));
        get_request_session(format!("example.com"), trace_headers, &session_store).unwrap();

        let mut trace_headers_two: HashMap<String, String> = HashMap::new();
        trace_headers_two.insert(format!("tracestate"), format!("serpress-session={}", name));
        get_request_session(format!("example.com"), trace_headers_two, &session_store).unwrap();
    }

    #[test]
    fn test_get_additional_headers() {
        let session_name = String::from("tiny-cow");
        let headers = HashMap::new();
        let add_headers = get_additional_headers(format!("https://tiny-cow.example.com/abc-xyz"), headers, &session_name, &format!("frontend"));

        assert_eq!(add_headers.get("traceparent").unwrap().len(), 55);
        assert_eq!(add_headers.get("tracestate").unwrap(), "serpress-session=tiny-cow,serpress-service=frontend");
        assert_eq!(add_headers.get("X-Forwarded-Host").unwrap(), "example.com");

        let mut already_headers : HashMap<String, String> = HashMap::new();
        already_headers.insert(format!("traceparent"), format!("anything"));
        already_headers.insert(format!("tracestate"), format!("serpress-session=tiny-cow"));
        already_headers.insert(format!("X-Forwarded-Host"), format!("example.com"));
        let add_headers = get_additional_headers(format!("https://abc.some-tunnel.com/abc-xyz"), already_headers, &session_name, &format!("frontend"));

        assert!(add_headers.get("traceparent").is_none());
        assert!(add_headers.get("X-Forwarded-Host").is_none());
        assert!(add_headers.get("tracestate").is_none());

        let mut already_headers_two : HashMap<String, String> = HashMap::new();
        already_headers_two.insert(format!("traceparent"), format!("anything"));
        already_headers_two.insert(format!("tracestate"), format!("other-service=32"));
        already_headers_two.insert(format!("X-Forwarded-Host"), format!("example.com"));
        let add_headers = get_additional_headers(format!("https://abc.some-tunnel.com/abc-xyz"), already_headers_two, &session_name, &format!("frontend"));

        assert!(add_headers.get("traceparent").is_none());
        assert!(add_headers.get("X-Forwarded-Host").is_none());
        assert_eq!(add_headers.get("tracestate").unwrap(), "other-service=32,serpress-session=tiny-cow,serpress-service=frontend");
    }

    #[test]
    fn test_get_target_domain() {
        let url1 = format!("tiny-cow.example.com/path/to/page.html");
        let url2 = format!("api.example.com");
        let url3 = format!("https://tiny-cow.example.com/a/b/c?a=b");

        assert_eq!(get_target_domain(&url1, &format!("tiny-cow")), "example.com");
        assert_eq!(get_target_domain(&url2, &format!("tiny-cow")), "api.example.com");
        assert_eq!(get_target_domain(&url3, &format!("tiny-cow")), "example.com");
    }
    
    #[test]
    fn test_get_target_url() {
        let session_store = MemorySessionStore::new();

        let input_config = new_server_config(String::from(CONF_STR)).unwrap();

        let name = session_store.new(input_config, NameKind::Animal, None);
        
        let (name, config) = get_request_session(format!("{}.example.com", name), HashMap::new(), &session_store).unwrap();

        // Standard named subdomain
        assert_eq!(get_target_url(format!("http://{}.example.com/?a=b", &name), HashMap::new(), &config, &name).unwrap(), (format!("http://localhost:8000/?a=b"), format!("frontend")));
        // With path
        assert_eq!(get_target_url(format!("http://{}.example.com/a/b/c/?a=b", &name), HashMap::new(), &config, &name).unwrap(), (format!("http://localhost:8000/a/b/c/?a=b"), format!("frontend")));
        // Test path_modifiers
        assert_eq!(get_target_url(format!("http://{}.example.com/foo/b/c/?a=b", &name), HashMap::new(), &config, &name).unwrap(), (format!("http://localhost:8000/bar/b/c/?a=b"), format!("frontend")));
        // Test domain routes
        assert_eq!(get_target_url(format!("http://{}.example.com/api/v1/?a=b", &name), HashMap::new(), &config, &name).unwrap(), (format!("http://localhost:8001/api/v1/?a=b"), format!("backend")));
        // Test no named subdomain
        assert_eq!(get_target_url(format!("http://api.example.com/api/v1/?a=b"), HashMap::new(), &config, &name).unwrap(), (format!("http://localhost:8001/api/v1/?a=b"), format!("backend")));
        // Test has already been through another serpress server
        let mut service_state_headers: HashMap<String, String> = HashMap::new();
        service_state_headers.insert(format!("tracestate"), format!("serpress-service={}", "frontend"));
        assert_eq!(get_target_url(format!("https://literally-any-url.com/foo/a/b"), service_state_headers, &config, &name).unwrap(), (format!("http://localhost:8000/foo/a/b"), format!("frontend")));
    }
}