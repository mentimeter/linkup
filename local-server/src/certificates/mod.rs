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
const LINKUP_CA_PEM_NAME: &str = "linkup_ca.cert.pem";

fn ca_cert_pem_path(certs_dir: &Path) -> PathBuf {
    certs_dir.join(LINKUP_CA_PEM_NAME)
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
    #[error("Missing NSS installation")]
    MissingNSS,
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
        if !is_nss_installed() {
            println!("It seems like you have Firefox installed.");
            println!(
            "For self-signed certificates to work with Firefox, you need to have nss installed."
        );
            let nss_url = if cfg!(target_os = "macos") {
                "`brew install nss`"
            } else {
                "`sudo apt install libnss3-tools`"
            };
            println!("You can install it with {}.", nss_url);
            println!("Please install it and then try to install local-dns again.");

            return Err(SetupError::MissingNSS);
        }

        add_ca_to_nss(certs_dir, &ff_cert_storages);
    }

    for domain in domains {
        create_domain_cert(certs_dir, &format!("*.{}", domain));
    }

    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum UninstallError {
    #[error("Failed to remove certs folder: {0}")]
    RemoveCertsFolder(String),
    #[error("Failed to remove CA certificate from keychain: {0}")]
    DeleteCaCertificate(String),
    #[error("Failed to refresh CA certificate registry: {0}")]
    RefreshCaCertificateRegistry(String),
}

pub fn uninstall_self_signed_certificates(certs_dir: &Path) -> Result<(), UninstallError> {
    if ca_exists_in_keychain() {
        remove_ca_from_keychain()?;
    }

    match std::fs::remove_dir_all(certs_dir) {
        Ok(_) => Ok(()),
        Err(error) => match error.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(UninstallError::RemoveCertsFolder(error.to_string())),
        },
    }
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

#[cfg(target_os = "macos")]
fn ca_exists_in_keychain() -> bool {
    process::Command::new("sudo")
        .arg("security")
        .arg("find-certificate")
        .arg("-c")
        .arg(LINKUP_CA_COMMON_NAME)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .expect("Failed to find linkup CA")
        .success()
}

#[cfg(target_os = "linux")]
fn ca_exists_in_keychain() -> bool {
    process::Command::new("find")
        .arg("/etc/ssl/certs")
        .arg("-iname")
        .arg(LINKUP_CA_PEM_NAME)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .expect("Failed to find linkup CA")
        .success()
}

#[cfg(target_os = "macos")]
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
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .expect("Failed to add CA to keychain");
}

#[cfg(target_os = "linux")]
fn add_ca_to_keychain(certs_dir: &Path) {
    process::Command::new("sudo")
        .arg("cp")
        .arg(ca_cert_pem_path(certs_dir))
        .arg("/usr/local/share/ca-certificates")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .expect("Failed to copy CA to /usr/local/share/ca-certificates");

    process::Command::new("sudo")
        .arg("update-ca-certificates")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .expect("Failed to update CA certificates");
}

#[cfg(target_os = "macos")]
fn remove_ca_from_keychain() -> Result<(), UninstallError> {
    let status = process::Command::new("sudo")
        .arg("security")
        .arg("delete-certificate")
        .arg("-t")
        .arg("-c")
        .arg(LINKUP_CA_COMMON_NAME)
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .map_err(|error| UninstallError::DeleteCaCertificate(error.to_string()))?;

    if !status.success() {
        return Err(UninstallError::DeleteCaCertificate(
            "security command returned unsuccessful exit status".to_string(),
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_ca_from_keychain() -> Result<(), UninstallError> {
    let status = process::Command::new("sudo")
        .arg("rm")
        .arg(format!(
            "/usr/local/share/ca-certificates/{}",
            LINKUP_CA_PEM_NAME
        ))
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .map_err(|error| UninstallError::DeleteCaCertificate(error.to_string()))?;

    if !status.success() {
        return Err(UninstallError::DeleteCaCertificate(
            "rm command returned unsuccessful exit status".to_string(),
        ));
    }

    process::Command::new("sudo")
        .arg("update-ca-certificates")
        .arg("--fresh")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .status()
        .map_err(|error| UninstallError::RefreshCaCertificateRegistry(error.to_string()))?
        .success()
        .then_some(())
        .ok_or_else(|| {
            UninstallError::RefreshCaCertificateRegistry(
                "update-ca-certificates command returned unsuccessful exit status".to_string(),
            )
        })
}

fn firefox_profiles_cert_storages() -> Vec<String> {
    #[cfg(target_os = "macos")]
    let profile_dirs = ["Library/Application Support/Firefox/Profiles"];

    #[cfg(target_os = "linux")]
    let profile_dirs = [
        ".mozilla/firefox",
        "snap/firefox/common/.mozilla/firefox",
        ".var/app/org.mozilla.firefox/.mozilla/firefox",
    ];

    let home = env::var("HOME").expect("Failed to get HOME env var");
    let mut storages: Vec<String> = Vec::new();

    for dir in profile_dirs
        .iter()
        .map(|dir| PathBuf::from(&home).join(dir))
        .map(fs::read_dir)
    {
        match dir {
            Ok(dir) => {
                for entry in dir.filter_map(|entry| entry.ok()) {
                    let path = entry.path();
                    if path.is_dir() {
                        if path.join("cert9.db").exists() {
                            storages.push(format!("{}{}", "sql:", path.to_str().unwrap()));
                        } else if path.join("cert8.db").exists() {
                            storages.push(format!("{}{}", "dmb:", path.to_str().unwrap()));
                        }
                    }
                }
            }

            Err(error) => {
                if !matches!(error.kind(), std::io::ErrorKind::NotFound) {
                    eprintln!("Failed to load Firefox profiles: {}", error);
                }
            }
        }
    }

    storages
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
            .arg(cert_storage)
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
