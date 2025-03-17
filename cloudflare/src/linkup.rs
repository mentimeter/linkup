use crate::{
    endpoints,
    framework::{self, response::ApiFailure},
};

pub async fn tunnel_prefix(
    client: &framework::async_api::Client,
    zone_id: &str,
) -> Result<String, ApiFailure> {
    let req = endpoints::zone::ZoneDetails {
        identifier: zone_id,
    };

    let zone = client.request(&req).await?;

    let zone_name = zone.result.name.replace(".", "-");
    let tunnel_name = format!("linkup-tunnel-{}-", zone_name);

    Ok(tunnel_name)
}
