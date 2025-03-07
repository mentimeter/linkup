use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls::crypto::ring::sign;
use rustls::pki_types::CertificateDer;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::{env, fs, path::PathBuf, process};

const LINKUP_CA_COMMON_NAME: &str = "Linkup Local CA";

pub fn ca_cert_pem_path(certs_dir: &PathBuf) -> PathBuf {
    certs_dir.join("linkup_ca.cert.pem")
}

pub fn ca_key_pem_path(certs_dir: &PathBuf) -> PathBuf {
    certs_dir.join("linkup_ca.key.pem")
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

    fn add_cert(&self, domain: &str, cert: CertifiedKey) {
        let mut certs = self.certs.write().unwrap();
        certs.insert(domain.to_string(), Arc::new(cert));
    }

    fn find_cert(&self, server_name: &str) -> Option<Arc<CertifiedKey>> {
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

        None
    }
}

impl ResolvesServerCert for WildcardSniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        if let Some(name) = client_hello.server_name() {
            return self.find_cert(name.as_ref());
        }

        None
    }
}

pub fn load_certificates_from_dir(cert_dir: &PathBuf) -> WildcardSniResolver {
    let resolver = WildcardSniResolver::new();

    let entries = fs::read_dir(cert_dir).expect("Failed to read certs directory");

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
                match load_cert_and_key(path, key_path) {
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

fn load_cert_and_key(
    cert_path: PathBuf,
    key_path: PathBuf,
) -> Result<CertifiedKey, Box<dyn std::error::Error>> {
    let cert_pem = fs::read(cert_path)?;
    let key_pem = fs::read(key_path)?;

    let certs = rustls_pemfile::certs(&mut &cert_pem[..])
        .filter_map(|cert| cert.ok())
        .map(CertificateDer::from)
        .collect::<Vec<CertificateDer<'static>>>();

    if certs.is_empty() {
        return Err("No valid certificates found".into());
    }

    let key_der =
        rustls_pemfile::private_key(&mut &key_pem[..])?.ok_or("No valid private key found")?;

    let signing_key =
        sign::any_supported_type(&key_der).map_err(|_| "Failed to parse signing key")?;

    Ok(CertifiedKey {
        cert: certs,
        key: signing_key,
        ocsp: None,
    })
}

pub fn create_domain_cert(certs_dir: &PathBuf, domain: &str) -> (Certificate, KeyPair) {
    let cert_pem_str = fs::read_to_string(ca_cert_pem_path(certs_dir)).unwrap();
    let key_pem_str = fs::read_to_string(ca_key_pem_path(certs_dir)).unwrap();

    let params = CertificateParams::from_ca_cert_pem(&cert_pem_str).unwrap();
    let ca_key = KeyPair::from_pem(&key_pem_str).unwrap();
    let ca_cert = params.self_signed(&ca_key).unwrap();

    let mut params = CertificateParams::new(vec![domain.to_string()]).unwrap();
    params.distinguished_name = DistinguishedName::new();
    params.distinguished_name.push(DnType::CommonName, domain);
    params.is_ca = rcgen::IsCa::NoCa;

    let key_pair = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key_pair, &ca_cert, &ca_key).unwrap();

    let escaped_domain = domain.replace("*", "wildcard_");
    let cert_path = certs_dir.join(format!("{}.cert.pem", &escaped_domain));
    let key_path = certs_dir.join(format!("{}.key.pem", &escaped_domain));
    fs::write(cert_path, cert.pem()).unwrap();
    fs::write(key_path, key_pair.serialize_pem()).unwrap();

    println!("Certificate for {} generated!", domain);

    (cert, key_pair)
}

pub fn upsert_ca_cert(certs_dir: &PathBuf) {
    if ca_cert_pem_path(certs_dir).exists() && ca_key_pem_path(certs_dir).exists() {
        return;
    }

    let mut params = CertificateParams::new(Vec::new()).unwrap();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];

    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, LINKUP_CA_COMMON_NAME);

    let key_pair = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key_pair).unwrap();

    fs::write(ca_cert_pem_path(certs_dir), cert.pem()).unwrap();
    fs::write(ca_key_pem_path(certs_dir), key_pair.serialize_pem()).unwrap();
}

pub fn add_ca_to_keychain(certs_dir: &PathBuf) {
    process::Command::new("sudo")
        .arg("security")
        .arg("add-trusted-cert")
        .arg("-d")
        .arg("-r")
        .arg("trustRoot")
        .arg("-k")
        .arg("/Library/Keychains/System.keychain")
        .arg(ca_cert_pem_path(certs_dir))
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn()
        .expect("Failed to add CA to keychain");
}

pub fn install_nss() {
    if is_nss_installed() {
        println!("NSS already installed, skipping installation");
        return;
    }

    let mut cmd = process::Command::new("brew")
        .arg("install")
        .arg("nss")
        .spawn()
        .expect("Failed to install NSS");

    cmd.wait().expect("Failed to wait for NSS install");
}

pub fn add_ca_to_nss(certs_dir: &PathBuf) {
    if !is_nss_installed() {
        println!("NSS not found, skipping CA installation");
        return;
    }

    let home = env::var("HOME").expect("Failed to get HOME env var");
    let firefox_profiles =
        fs::read_dir(PathBuf::from(home).join("Library/Application Support/Firefox/Profiles"))
            .expect("Failed to read Firefox profiles directory")
            .filter_map(|entry| {
                let entry = entry.expect("Failed to read Firefox profile dir entry entry");
                let path = entry.path();
                if path.is_dir() {
                    if path.join("cert9.db").exists() {
                        return Some(format!("{}{}", "sql:", path.to_str().unwrap()));
                    } else if path.join("cert8.db").exists() {
                        return Some(format!("{}{}", "dmb:", path.to_str().unwrap()));
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<String>>();

    for profile in firefox_profiles {
        process::Command::new("certutil")
            .arg("-A")
            .arg("-d")
            .arg(profile)
            .arg("-t")
            .arg("C,,")
            .arg("-n")
            .arg(LINKUP_CA_COMMON_NAME)
            .arg("-i")
            .arg(ca_cert_pem_path(certs_dir))
            .spawn()
            .expect("Failed to add CA to NSS");
    }
}

fn is_nss_installed() -> bool {
    let res = process::Command::new("which")
        .args(["certutil"])
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .stdin(process::Stdio::null())
        .status()
        .unwrap();

    res.success()
}
