---
title: Deploy Linkup to Cloudflare
description: Deploy the remote linkup worker to a Cloudflare domain
---

## Prerequisites

- A Cloudflare account
- A domain connected to your Cloudflare account that you can use for linkup

## Deploying Linkup

In order to run linkup sessions, you need:

- A dedicated domain for the linkup cloudflare worker to run on
- Deployed copies of the remote services you want to provide

### Running the deploy command

Linkup comes with a `linkup deploy` command that can deploy all of the required infrastructure components to Cloudflare on your behalf.

What you will need to run the `linkup deploy` command is:

- Your cloudflare account ID
- The ID of the zone you want to deploy to
- Your personal global CloudFlare API token that gives linkup the permissions to deploy these resources

```
linkup deploy --help
Deploy services to Cloudflare

Usage: linkup deploy --email <EMAIL> --api-key <API_KEY> --account-id <ACCOUNT_ID> --zone-ids <ZONE_IDS>...

Options:
  -e, --email <EMAIL>            Cloudflare user email
  -k, --api-key <API_KEY>        Cloudflare user global API Key
  -a, --account-id <ACCOUNT_ID>  Cloudflare account ID
  -z, --zone-ids <ZONE_IDS>...   Cloudflare zone IDs
  -h, --help                     Print help
```

### Tearing down / linkup destroy

You can clean up all of the resources that linkup has deployed by running the `linkup destroy` command, which needs the same set of arguments as the `linkup deploy` command.

```
linkup destroy --help
Destroy/remove linkup installation from Cloudflare

Usage: linkup destroy --email <EMAIL> --api-key <API_KEY> --account-id <ACCOUNT_ID> --zone-ids <ZONE_IDS>...

Options:
  -e, --email <EMAIL>            Cloudflare user email
  -k, --api-key <API_KEY>        Cloudflare user global API Key
  -a, --account-id <ACCOUNT_ID>  Cloudflare account ID
  -z, --zone-ids <ZONE_IDS>...   Cloudflare zone IDs
  -h, --help                     Print help
```
