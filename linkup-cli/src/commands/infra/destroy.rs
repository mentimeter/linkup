use anyhow::Result;

use super::cloudflare::{
    api::{AccountCloudflareApi, CloudflareApi},
    auth,
    resources::{TargetCfResources, cf_resources},
};
use super::{
    Args as InfraArgs,
    notifier::{ConsoleNotifier, DeployNotifier},
};

#[derive(clap::Args)]
pub struct DestroyArgs {}

pub async fn destroy(_args: &DestroyArgs, infra_args: &InfraArgs) -> Result<()> {
    println!("Destroying from Cloudflare...");
    println!("Account ID: {}", infra_args.account_id);
    println!("Zone IDs: {:?}", infra_args.zone_ids);

    let auth =
        auth::CloudflareGlobalTokenAuth::new(infra_args.api_key.clone(), infra_args.email.clone());
    let zone_ids_strings: Vec<String> = infra_args.zone_ids.iter().map(|s| s.to_string()).collect();

    let cloudflare_api = AccountCloudflareApi::new(
        infra_args.account_id.to_string(),
        zone_ids_strings.clone(),
        Box::new(auth),
    );

    let cloudflare_client = cloudflare::framework::async_api::Client::new(
        cloudflare::framework::auth::Credentials::UserAuthKey {
            email: infra_args.email.clone(),
            key: infra_args.api_key.clone(),
        },
        cloudflare::framework::HttpApiClientConfig::default(),
        cloudflare::framework::Environment::Production,
    )
    .expect("Cloudflare API Client to have been created");

    let notifier = ConsoleNotifier::new();

    let mut zone_names = Vec::with_capacity(zone_ids_strings.len());
    for zone_id in zone_ids_strings {
        let zone_name = cloudflare_api.get_zone_name(&zone_id).await?;
        zone_names.push(zone_name);
    }

    let resources = cf_resources(
        infra_args.account_id.clone(),
        infra_args.zone_ids[0].clone(),
        zone_names[0].clone(),
        &zone_names,
        &infra_args.zone_ids,
    );

    destroy_from_cloudflare(&resources, &cloudflare_api, &cloudflare_client, &notifier).await?;

    Ok(())
}

pub async fn destroy_from_cloudflare(
    resources: &TargetCfResources,
    api: &impl CloudflareApi,
    cloudflare_client: &cloudflare::framework::async_api::Client,
    notifier: &impl DeployNotifier,
) -> Result<()> {
    // 1) Check which resources actually exist and need removal
    let plan = resources.check_destroy_plan(api).await?;

    // 2) If nothing to remove, just notify and return
    if plan.is_empty() {
        notifier.notify("No resources to remove! Everything is already gone.");
        return Ok(());
    }

    // 3) Otherwise, show or summarize the plan, ask user confirmation
    notifier.notify("The following resources will be removed:");
    // You can do something fancier; here we just debug-print
    notifier.notify(&format!("{:#?}", plan));

    if !notifier.ask_confirmation() {
        notifier.notify("Destroy canceled by user.");
        return Ok(());
    }

    // 4) Execute the plan
    resources
        .execute_destroy_plan(api, cloudflare_client, &plan, notifier)
        .await?;

    notifier.notify("Destroy completed successfully.");

    Ok(())
}
