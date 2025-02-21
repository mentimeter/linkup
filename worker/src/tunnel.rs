use std::fmt::Display;

use crate::TunnelData;

#[derive(Debug)]
pub enum CreateTunnelError {
    CreateCloudflareTunnel(String),
    CreateDNS(String),
    FetchZone(String),
}

impl Display for CreateTunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateTunnelError::CreateCloudflareTunnel(text) => {
                write!(f, "Failed to crate tunnel: {}", text)
            }
            CreateTunnelError::CreateDNS(text) => write!(f, "Failed to crate DNS record: {}", text),
            CreateTunnelError::FetchZone(text) => {
                write!(f, "Failed to fetch Zone details: {}", text)
            }
        }
    }
}

impl std::error::Error for CreateTunnelError {}

pub async fn create_tunnel(
    api_token: &str,
    account_id: &str,
    zone_id: &str,
    tunnel_name: &str,
) -> Result<TunnelData, CreateTunnelError> {
    let client = crate::cloudflare_client(api_token);
    let tunnel_secret = crate::generate_secret();

    let create_tunnel_req = cloudflare::endpoints::cfd_tunnel::create_tunnel::CreateTunnel {
        account_identifier: account_id,
        params: cloudflare::endpoints::cfd_tunnel::create_tunnel::Params {
            name: tunnel_name,
            tunnel_secret: &tunnel_secret.as_bytes().to_vec(),
            config_src: &cloudflare::endpoints::cfd_tunnel::ConfigurationSrc::Local,
            metadata: None,
        },
    };

    let tunnel = client
        .request(&create_tunnel_req)
        .await
        .map_err(|err| CreateTunnelError::CreateCloudflareTunnel(err.to_string()))?
        .result;

    let create_dns_req = cloudflare::endpoints::dns::CreateDnsRecord {
        zone_identifier: zone_id,
        params: cloudflare::endpoints::dns::CreateDnsRecordParams {
            proxied: Some(true),
            name: tunnel_name,
            content: cloudflare::endpoints::dns::DnsContent::CNAME {
                content: format!("{}.cfargotunnel.com", tunnel.id),
            },
            ttl: None,
            priority: None,
        },
    };

    client
        .request(&create_dns_req)
        .await
        .map_err(|err| CreateTunnelError::CreateDNS(err.to_string()))?;

    let get_zone_req = cloudflare::endpoints::zone::ZoneDetails {
        identifier: zone_id,
    };

    let zone = client
        .request(&get_zone_req)
        .await
        .map_err(|err| CreateTunnelError::FetchZone(err.to_string()))?
        .result;

    let tunnel_data = TunnelData {
        account_id: account_id.to_string(),
        name: tunnel_name.to_string(),
        url: format!("https://{}.{}", &tunnel_name, &zone.name),
        id: tunnel.id.to_string(),
        secret: tunnel_secret,
        last_started: worker::Date::now().as_millis(),
    };

    Ok(tunnel_data)
}

#[derive(Debug)]
pub enum DeleteTunnelError {
    DeleteCloudflareTunnel(String),
    GetDNSRecord(String),
    DeleteDNSRecord(String),
}

impl Display for DeleteTunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeleteTunnelError::DeleteCloudflareTunnel(text) => {
                write!(f, "Failed to delete tunnel: {}", text)
            }
            DeleteTunnelError::GetDNSRecord(text) => {
                write!(f, "Failed to fetch DNS record: {}", text)
            }
            DeleteTunnelError::DeleteDNSRecord(text) => {
                write!(f, "Failed to delete DNS record: {}", text)
            }
        }
    }
}

impl std::error::Error for DeleteTunnelError {}

pub async fn delete_tunnel(
    api_token: &str,
    account_id: &str,
    zone_id: &str,
    tunnel_id: &str,
) -> Result<(), DeleteTunnelError> {
    let client = crate::cloudflare_client(api_token);

    let delete_tunnel_req = cloudflare::endpoints::cfd_tunnel::delete_tunnel::DeleteTunnel {
        account_identifier: account_id,
        tunnel_id,
        params: cloudflare::endpoints::cfd_tunnel::delete_tunnel::Params { cascade: true },
    };

    client
        .request(&delete_tunnel_req)
        .await
        .map_err(|error| DeleteTunnelError::DeleteCloudflareTunnel(error.to_string()))?;

    let get_dns_record_req = cloudflare::endpoints::dns::ListDnsRecords {
        zone_identifier: zone_id,
        params: cloudflare::endpoints::dns::ListDnsRecordsParams {
            record_type: Some(cloudflare::endpoints::dns::DnsContent::CNAME {
                content: format!("{}.cfargotunnel.com", tunnel_id),
            }),
            ..Default::default()
        },
    };

    let records = client
        .request(&get_dns_record_req)
        .await
        .map_err(|error| DeleteTunnelError::GetDNSRecord(error.to_string()))?
        .result;

    let record = match records.len() {
        0 => {
            return Err(DeleteTunnelError::GetDNSRecord(
                "Fetching DNS for tunnel returned empty".to_string(),
            ))
        }
        1 => &records[0],
        2.. => {
            return Err(DeleteTunnelError::GetDNSRecord(
                "Fetching DNS for tunnel returned more than one record".to_string(),
            ))
        }
    };

    let delete_dns_record_red = cloudflare::endpoints::dns::DeleteDnsRecord {
        zone_identifier: zone_id,
        identifier: &record.id,
    };

    client
        .request(&delete_dns_record_red)
        .await
        .map_err(|error| DeleteTunnelError::DeleteDNSRecord(error.to_string()))?;

    Ok(())
}
