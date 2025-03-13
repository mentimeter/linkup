mod wildcard_sni_resolver;

use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls::crypto::ring::sign;
use rustls::pki_types::CertificateDer;
use rustls::sign::CertifiedKey;
use std::{
    env,
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    process,
};

pub use wildcard_sni_resolver::WildcardSniResolver;

const LINKUP_CA_COMMON_NAME: &str = "Linkup Local CA";

fn ca_cert_pem_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join("linkup_ca.cert.pem")
}

fn ca_key_pem_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join("linkup_ca.key.pem")
}

#[derive(Debug, thiserror::Error)]
pub enum BuildCertifiedKeyError {
    #[error("Failed to read file: {0}")]
    FileRead(#[from] std::io::Error),
    #[error("File does not contain valid certificate")]
    InvalidCertFile,
    #[error("File does not contain valid private key")]
    InvalidKeyFile,
}

fn build_certified_key(
    cert_path: &Path,
    key_path: &Path,
) -> Result<CertifiedKey, BuildCertifiedKeyError> {
    let mut cert_pem = BufReader::new(File::open(cert_path)?);
    let mut key_pem = BufReader::new(File::open(key_path)?);

    let certs = rustls_pemfile::certs(&mut cert_pem)
        .filter_map(|cert| cert.ok())
        .collect::<Vec<CertificateDer<'static>>>();

    if certs.is_empty() {
        return Err(BuildCertifiedKeyError::InvalidCertFile);
    }

    let key_der = rustls_pemfile::private_key(&mut key_pem)
        .map_err(|_| BuildCertifiedKeyError::InvalidKeyFile)?
        .ok_or(BuildCertifiedKeyError::InvalidCertFile)?;

    let signing_key =
        sign::any_supported_type(&key_der).map_err(|_| BuildCertifiedKeyError::InvalidKeyFile)?;

    Ok(CertifiedKey {
        cert: certs,
        key: signing_key,
        ocsp: None,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("Failed to create certificates directory '{0}': {1}")]
    CreateCertsDir(PathBuf, String),
}

pub fn setup_self_signed_certificates(
    certs_dir: &Path,
    domains: &[String],
) -> Result<(), SetupError> {
    if !certs_dir.exists() {
        fs::create_dir_all(certs_dir).map_err(|error| {
            SetupError::CreateCertsDir(certs_dir.to_path_buf(), error.to_string())
        })?;
    }

    upsert_ca_cert(certs_dir);
    add_ca_to_keychain(certs_dir);

    let ff_cert_storages = firefox_profiles_cert_storages();
    if !ff_cert_storages.is_empty() {
        install_nss();
        add_ca_to_nss(certs_dir, &ff_cert_storages);
    }

    for domain in domains {
        create_domain_cert(&certs_dir, &format!("*.{}", domain));
    }

    Ok(())
}

pub fn create_domain_cert(certs_dir: &Path, domain: &str) -> (Certificate, KeyPair) {
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

    (cert, key_pair)
}

fn upsert_ca_cert(certs_dir: &Path) {
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

fn add_ca_to_keychain(certs_dir: &Path) {
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
        .status()
        .expect("Failed to add CA to keychain");
}

fn firefox_profiles_cert_storages() -> Vec<String> {
    let home = env::var("HOME").expect("Failed to get HOME env var");

    match fs::read_dir(PathBuf::from(home).join("Library/Application Support/Firefox/Profiles")) {
        Ok(dir) => dir
            .filter_map(|entry| {
                let entry = entry.expect("Failed to read Firefox profile dir entry entry");
                let path = entry.path();
                if path.is_dir() {
                    if path.join("cert9.db").exists() {
                        Some(format!("{}{}", "sql:", path.to_str().unwrap()))
                    } else if path.join("cert8.db").exists() {
                        Some(format!("{}{}", "dmb:", path.to_str().unwrap()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<String>>(),
        Err(error) => {
            eprintln!("Failed to load Firefox profiles: {}", error);

            Vec::new()
        }
    }
}

fn install_nss() {
    if is_nss_installed() {
        println!("NSS already installed, skipping installation");
        return;
    }

    process::Command::new("brew")
        .arg("install")
        .arg("nss")
        .status()
        .expect("Failed to install NSS");
}

fn add_ca_to_nss(certs_dir: &Path, cert_storages: &[String]) {
    if !is_nss_installed() {
        println!("NSS not found, skipping CA installation");
        return;
    }

    for cert_storage in cert_storages {
        let result = process::Command::new("certutil")
            .arg("-A")
            .arg("-d")
            .arg(&cert_storage)
            .arg("-t")
            .arg("C,,")
            .arg("-n")
            .arg(LINKUP_CA_COMMON_NAME)
            .arg("-i")
            .arg(ca_cert_pem_path(certs_dir))
            .status();

        if let Err(e) = result {
            eprintln!("certutil failed to run for profile {}: {}", cert_storage, e);
        }
    }
}

fn is_nss_installed() -> bool {
    let res = process::Command::new("which")
        .args(["certutil"])
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .stdin(process::Stdio::null())
        .status();

    match res {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("Failed to check if certutil is installed: {}", e);
            false
        }
    }
}
