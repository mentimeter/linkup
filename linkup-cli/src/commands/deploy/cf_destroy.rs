use std::env;

use crate::commands::deploy::{
    api::AccountCloudflareApi, auth::CloudflareGlobalTokenAuth, console_notify::ConsoleNotifier,
    resources::cf_resources,
};

use super::{
    api::CloudflareApi, cf_deploy::DeployNotifier, resources::TargetCfResources, DeployArgs,
    DeployError,
};

#[derive(clap::Args)]
pub struct DestroyArgs {
    #[arg(
        short = 'a',
        long = "account-id",
        help = "Cloudflare account ID",
        value_name = "ACCOUNT_ID"
    )]
    account_id: String,

    #[arg(
        short = 'z',
        long = "zone-ids",
        help = "Cloudflare zone IDs",
        value_name = "ZONE_IDS",
        num_args = 1..,
        required = true
    )]
    zone_ids: Vec<String>,
}

pub async fn destroy(args: &DestroyArgs) -> Result<(), DeployError> {
    println!("Destroying from Cloudflare...");
    println!("Account ID: {}", args.account_id);
    println!("Zone IDs: {:?}", args.zone_ids);

    let api_key = env::var("CLOUDFLARE_API_KEY").expect("Missing Cloudflare API token");
    let email = env::var("CLOUDFLARE_EMAIL").expect("Missing Cloudflare email");
    let zone_ids_strings: Vec<String> = args.zone_ids.iter().map(|s| s.to_string()).collect();

    // let token_auth = CloudflareTokenAuth::new(api_key);
    let global_key_auth = CloudflareGlobalTokenAuth::new(api_key, email);

    let cloudflare_api = AccountCloudflareApi::new(
        args.account_id.to_string(),
        zone_ids_strings,
        Box::new(global_key_auth),
    );
    let notifier = ConsoleNotifier::new();

    let resources = cf_resources();

    destroy_from_cloudflare(&resources, &cloudflare_api, &notifier).await?;

    Ok(())
}

pub async fn destroy_from_cloudflare(
    resources: &TargetCfResources,
    api: &impl CloudflareApi,
    notifier: &impl DeployNotifier,
) -> Result<(), DeployError> {
    let script_name = &resources.worker_script_name;
    let kv_name = &resources.kv_name;

    // For each zone, remove routes and DNS records
    for zone_id in api.zone_ids() {
        for route_config in &resources.zone_resources.routes {
            let route = &route_config.route;
            let script = &route_config.script;

            let zone_name = api.get_zone_name(zone_id.clone()).await?;

            // Remove Worker route if it exists
            let existing_route = api
                .get_worker_route(
                    zone_id.clone(),
                    route_config.worker_route(zone_name.clone()),
                    script.clone(),
                )
                .await?;
            if let Some(route_id) = existing_route {
                notifier.notify(&format!(
                    "Removing worker route for pattern '{}' and script '{}' in zone '{}'.",
                    route, script, zone_id
                ));
                api.remove_worker_route(zone_id.clone(), route_id).await?;
                notifier.notify(&format!(
                    "Worker route for pattern '{}' and script '{}' removed.",
                    route, script
                ));
            } else {
                notifier.notify(&format!(
                    "No worker route for pattern '{}' and script '{}' found in zone '{}', nothing to remove.",
                    route, script, zone_id
                ));
            }
        }
        for dns_record in &resources.zone_resources.dns_records {
            let route = &dns_record.route;
            // Remove DNS record if it exists
            let existing_dns = api
                .get_dns_record(zone_id.clone(), dns_record.comment())
                .await?;
            if let Some(record) = existing_dns {
                notifier.notify(&format!(
                    "Removing DNS record '{}' in zone '{}'.",
                    record.name, zone_id
                ));
                api.remove_dns_record(zone_id.clone(), record.id.clone())
                    .await?;
                notifier.notify(&format!("DNS record '{}' removed.", record.name));
            } else {
                notifier.notify(&format!(
                    "No DNS record for '{}' found in zone '{}', nothing to remove.",
                    route, zone_id
                ));
            }
        }
    }

    // Remove the Worker script - must happen before kv delete
    let existing_info = api.get_worker_script_info(script_name.clone()).await?;
    if existing_info.is_some() {
        notifier.notify(&format!("Removing worker script '{}'...", script_name));
        api.remove_worker_script(script_name.to_string()).await?;
        notifier.notify("Worker script removed successfully.");
    } else {
        notifier.notify(&format!(
            "Worker script '{}' does not exist, nothing to remove.",
            script_name
        ));
    }

    // Remove the KV namespace if it exists
    let kv_ns_id = api.get_kv_namespace_id(kv_name.clone()).await?;
    if let Some(ns_id) = kv_ns_id {
        notifier.notify(&format!("Removing KV namespace '{}'...", kv_name));
        api.remove_kv_namespace(ns_id.clone()).await?;
        notifier.notify(&format!("KV namespace '{}' removed successfully.", kv_name));
    } else {
        notifier.notify(&format!(
            "KV namespace '{}' does not exist, nothing to remove.",
            kv_name
        ));
    }

    for zone_id in api.zone_ids() {
        let cache_name = resources.zone_resources.cache_rules.name.clone();
        let cache_phase = resources.zone_resources.cache_rules.phase.clone();
        let cache_ruleset = api
            .get_ruleset(zone_id.clone(), cache_name.clone(), cache_phase.clone())
            .await?;
        if let Some(ruleset_id) = cache_ruleset {
            notifier.notify(&format!(
                "Removing cache ruleset '{}' in zone '{}'.",
                cache_name, zone_id
            ));
            api.remove_ruleset_rules(zone_id.clone(), ruleset_id.clone())
                .await?;
            notifier.notify(&format!("Cache ruleset '{}' removed.", cache_name));
        } else {
            notifier.notify(&format!(
                "Cache ruleset '{}' does not exist in zone '{}', nothing to remove.",
                cache_name, zone_id
            ));
        }
    }

    Ok(())
}
