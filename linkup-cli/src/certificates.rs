use std::{fs, io::BufReader, path::PathBuf, process};

use rcgen::{Certificate, CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls_pemfile::certs;

use crate::{linkup_certs_dir_path, linkup_dir_path};

pub fn ca_cert_pem_path() -> PathBuf {
    linkup_certs_dir_path().join("mentimeter_ca.cert.pem")
}

pub fn ca_key_pem_path() -> PathBuf {
    linkup_certs_dir_path().join("mentimeter_ca.key.pem")
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
    if let Some(cert_pair) = get_cert_pair("mentimeter_ca") {
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
        .push(rcgen::DnType::CommonName, "Mentimeter Local CA");

    let key_pair = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key_pair).unwrap();

    fs::write(ca_cert_pem_path(), cert.pem()).unwrap();
    fs::write(ca_key_pem_path(), key_pair.serialize_pem()).unwrap();

    (cert, key_pair)
}

pub async fn add_ca_to_keychain() {
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
