use crate::certificates::build_certified_key;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, thiserror::Error)]
pub enum WildcardSniResolverError {
    #[error("Failed to read certs directory: {0}")]
    ReadDir(#[from] std::io::Error),

    #[error("Failed to get file name")]
    FileName,

    #[error("Error building certified key: {0}")]
    LoadCert(#[from] crate::certificates::BuildCertifiedKeyError),
}

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

    pub fn load_dir(certs_dir: &Path) -> Result<Self, WildcardSniResolverError> {
        let resolver = WildcardSniResolver::new();

        let entries = match fs::read_dir(certs_dir) {
            Ok(entries) => entries,
            Err(error) => match error.kind() {
                std::io::ErrorKind::NotFound => return Ok(resolver),
                _ => return Err(error.into()),
            },
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                let file_name = file_name.to_string_lossy();

                if file_name.contains(".cert.pem") && !path.starts_with("linkup_ca") {
                    let domain_name = file_name.replace(".cert.pem", "").replace("wildcard_", "*");
                    let key_path =
                        PathBuf::from(path.to_string_lossy().replace(".cert.pem", ".key.pem"));

                    if key_path.exists() {
                        match build_certified_key(&path, &key_path) {
                            Ok(certified_key) => {
                                resolver.add_cert(&domain_name, certified_key);
                            }
                            Err(e) => {
                                eprintln!("Error loading cert/key for {domain_name}: {e}");
                            }
                        }
                    }
                }
            }
        }

        Ok(resolver)
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
