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

Linkup comes with a `linkup infra deploy` command that can deploy all of the required infrastructure components to Cloudflare on your behalf.

To run it you will need:

- Your Cloudflare account ID
- The ID of the zone you want to deploy to
- Your Cloudflare email and global API key, which give Linkup the permissions to deploy these resources

```
linkup infra --help

Usage: linkup infra --email <EMAIL> --api-key <API_KEY> --account-id <ACCOUNT_ID> --zone-ids <ZONE_IDS>... <COMMAND>

Commands:
  deploy   Deploy services to Cloudflare
  destroy  Destroy/remove linkup installation from Cloudflare

Options:
  -e, --email <EMAIL>            Cloudflare user email
  -k, --api-key <API_KEY>        Cloudflare user global API Key
  -a, --account-id <ACCOUNT_ID>  Cloudflare account ID
  -z, --zone-ids <ZONE_IDS>...   Cloudflare zone IDs
  -h, --help                     Print help
```

So the full command looks like:

```sh
linkup infra \
  --email you@example.com \
  --api-key <api-key> \
  --account-id <account-id> \
  --zone-ids <zone-id> \
  deploy
```

### Tearing down / linkup infra destroy

You can clean up all of the resources Linkup has deployed by running `linkup infra destroy` with the same Cloudflare credentials:

```sh
linkup infra \
  --email you@example.com \
  --api-key <api-key> \
  --account-id <account-id> \
  --zone-ids <zone-id> \
  destroy
```
