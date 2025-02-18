use std::fmt::Display;

use crate::TunnelData;

#[derive(Debug)]
pub enum Error {
    CreateCloudflareTunnel(String),
    CreateDNS(String),
    FetchZone(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CreateCloudflareTunnel(text) => write!(f, "Failed to crate tunnel: {}", text),
            Error::CreateDNS(text) => write!(f, "Failed to crate DNS record: {}", text),
            Error::FetchZone(text) => write!(f, "Failed to fetch Zone details: {}", text),
        }
    }
}

impl std::error::Error for Error {}

pub async fn create_tunnel(
    api_token: &str,
    account_id: &str,
    zone_id: &str,
    tunnel_name: &str,
) -> Result<TunnelData, Error> {
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
        .map_err(|err| Error::CreateCloudflareTunnel(err.to_string()))?
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
        .map_err(|err| Error::CreateDNS(err.to_string()))?;

    let get_zone_req = cloudflare::endpoints::zone::ZoneDetails {
        identifier: zone_id,
    };

    let zone = client
        .request(&get_zone_req)
        .await
        .map_err(|err| Error::FetchZone(err.to_string()))?
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
