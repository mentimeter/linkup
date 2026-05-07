---
title: Deploy Linkup to Cloudflare
description: Deploy the remote linkup worker to a Cloudflare domain
---

## Prerequisites

- A Cloudflare account
- A domain connected to your Cloudflare account that you can use for linkup

## Deploying Linkup

To run linkup sessions, you need:

- A dedicated domain for the linkup cloudflare worker to run on
- Deployed copies of the remote services you want to provide

### Running the deploy command

Linkup comes with a `linkup infra deploy` command that can deploy all of the
required infrastructure components to Cloudflare on your behalf.

To run it you will need:

- Your Cloudflare account ID
- The ID(s) of the zone(s) you want to deploy to
- Your Cloudflare email and global API key, which give Linkup the permissions to
  deploy these resources

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

### Multiple zones and the tunnel zone

`--zone-ids` accepts more than one ID. The Linkup worker is a single script at
the account level. For each zone you list, Linkup adds the routes and DNS
records that send the zone's traffic to that worker. The public hostnames in
your `domains` config can come from any of those zones.

The first zone in the list is treated specially as the _tunnel zone_. All
Cloudflare tunnel DNS records the worker provisions for live sessions
(`linkup-tunnel-<zone>-<session>.<tunnel-zone-domain>`) are created there, and
the cache rule that excludes those tunnel hostnames from Cloudflare's cache is
installed there too. The remaining zones only get their wildcard DNS records and
worker routes.

The order of zones after the first doesn't matter. Only the first slot is
meaningful, so pick a zone whose root domain you're happy to use for tunnel DNS:

```sh
linkup infra \
  --email you@example.com \
  --api-key <api-key> \
  --account-id <account-id> \
  --zone-ids <tunnel-zone-id> <other-zone-id> <another-zone-id> \
  deploy
```

### Tearing down / linkup infra destroy

You can clean up all of the resources Linkup has deployed by running
`linkup infra destroy` with the same Cloudflare credentials:

```sh
linkup infra \
  --email you@example.com \
  --api-key <api-key> \
  --account-id <account-id> \
  --zone-ids <zone-id> \
  destroy
```
