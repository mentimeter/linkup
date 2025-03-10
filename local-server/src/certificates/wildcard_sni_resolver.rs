use crate::certificates::load_cert_and_key;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct WildcardSniResolver {
    certs: RwLock<HashMap<String, Arc<CertifiedKey>>>,
}

impl WildcardSniResolver {
    fn new() -> Self {
        Self {
            certs: RwLock::new(HashMap::new()),
        }
    }

    pub fn load_dir(certs_dir: &Path) -> Self {
        let resolver = WildcardSniResolver::new();

        let entries = fs::read_dir(certs_dir).expect("Failed to read certs directory");

        for entry in entries.flatten() {
            let path = entry.path();

            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".cert.pem")
                && !path.starts_with("linkup_ca")
            {
                let path_str = path.to_string_lossy();
                let domain_name = path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .replace(".cert.pem", "")
                    .replace("wildcard_", "*");
                let key_path = PathBuf::from(path_str.replace(".cert.pem", ".key.pem"));

                if key_path.exists() {
                    match load_cert_and_key(&path, &key_path) {
                        Ok(certified_key) => {
                            println!("Loaded certificate for {}", domain_name);
                            resolver.add_cert(&domain_name, certified_key);
                        }
                        Err(e) => {
                            eprintln!("Error loading cert/key for {domain_name}: {e}");
                        }
                    }
                }
            }
        }

        resolver
    }

    fn add_cert(&self, domain: &str, cert: CertifiedKey) {
        let mut certs = self.certs.write().unwrap();
        certs.insert(domain.to_string(), Arc::new(cert));
    }
}

impl ResolvesServerCert for WildcardSniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        if let Some(server_name) = client_hello.server_name() {
            let certs = self.certs.read().unwrap();

            if let Some(cert) = certs.get(server_name) {
                return Some(cert.clone());
            }

            let parts: Vec<&str> = server_name.split('.').collect();

            for i in 0..parts.len() {
                let wildcard_domain = format!("*.{}", parts[i..].join("."));
                if let Some(cert) = certs.get(&wildcard_domain) {
                    return Some(cert.clone());
                }
            }
        }

        None
    }
}
