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
    resources.execute_destroy_plan(api, &plan, notifier).await?;

    notifier.notify("Destroy completed successfully.");

    Ok(())
}
