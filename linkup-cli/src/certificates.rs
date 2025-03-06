use std::{env, fs, path::PathBuf, process};

use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair};

use crate::linkup_certs_dir_path;

const LINKUP_CA_COMMON_NAME: &str = "Linkup Local CA";

pub fn ca_cert_pem_path() -> PathBuf {
    linkup_certs_dir_path().join("linkup_ca.cert.pem")
}

pub fn ca_key_pem_path() -> PathBuf {
    linkup_certs_dir_path().join("linkup_ca.key.pem")
}

pub fn get_cert_pair(domain: &str) -> Option<(Certificate, KeyPair)> {
    let escaped_domain = domain.replace("*", "wildcard_");
    let cert_path = linkup_certs_dir_path().join(format!("{}.cert.pem", &escaped_domain));
    let key_path = linkup_certs_dir_path().join(format!("{}.key.pem", &escaped_domain));

    if !cert_path.exists() || !key_path.exists() {
        return None;
    }

    let cert_pem_str = fs::read_to_string(cert_path).unwrap();
    let key_pem_str = fs::read_to_string(key_path).unwrap();

    let params = CertificateParams::from_ca_cert_pem(&cert_pem_str).unwrap();
    let key_pair = KeyPair::from_pem(&key_pem_str).unwrap();
    let cert = params.self_signed(&key_pair).unwrap();

    Some((cert, key_pair))
}

pub fn create_domain_cert(domain: &str) -> (Certificate, KeyPair) {
    let cert_pem_str = fs::read_to_string(ca_cert_pem_path()).unwrap();
    let key_pem_str = fs::read_to_string(ca_key_pem_path()).unwrap();

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
    let cert_path = linkup_certs_dir_path().join(format!("{}.cert.pem", &escaped_domain));
    let key_path = linkup_certs_dir_path().join(format!("{}.key.pem", &escaped_domain));
    fs::write(cert_path, cert.pem()).unwrap();
    fs::write(key_path, key_pair.serialize_pem()).unwrap();

    println!("Certificate for {} generated!", domain);

    (cert, key_pair)
}

/// Return if a new certificate/keypair was generated
pub fn upsert_ca_cert() -> (Certificate, KeyPair) {
    if let Some(cert_pair) = get_cert_pair("linkup_ca") {
        return cert_pair;
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

    fs::write(ca_cert_pem_path(), cert.pem()).unwrap();
    fs::write(ca_key_pem_path(), key_pair.serialize_pem()).unwrap();

    (cert, key_pair)
}

pub fn add_ca_to_keychain() {
    process::Command::new("sudo")
        .arg("security")
        .arg("add-trusted-cert")
        .arg("-d")
        .arg("-r")
        .arg("trustRoot")
        .arg("-k")
        .arg("/Library/Keychains/System.keychain")
        .arg(ca_cert_pem_path())
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

pub fn add_ca_to_nss() {
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
            .arg(ca_cert_pem_path())
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
