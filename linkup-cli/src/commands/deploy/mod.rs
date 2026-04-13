mod api;
mod auth;
mod cf_deploy;
mod cf_destroy;
mod console_notify;
mod resources;

pub use cf_deploy::{DeployArgs, DeployError, deploy};
pub use cf_destroy::{DestroyArgs, destroy};

// TODO(augustoccesar)[2026-04-13]: This function is duplicated on workers/src/tunnel.rs.
//  We can probably find a place to unify them.
pub async fn tunnel_prefix(
    client: &cloudflare::framework::client::async_api::Client,
    zone_id: &str,
) -> Result<String, cloudflare::framework::response::ApiFailure> {
    let req = cloudflare::endpoints::zones::zone::ZoneDetails {
        identifier: zone_id,
    };

    let zone = client.request(&req).await?;

    let zone_name = zone.result.name.replace(".", "-");
    let tunnel_name = format!("linkup-tunnel-{}-", zone_name);

    Ok(tunnel_name)
}
