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

### Configuring the domain & worker

Linkup is deployed as a cloudflare worker with a key-value store, and can be deployed using the wrangler cli:

```sh
cd worker
cp wrangler.toml.sample wrangler.toml
# Edit wrangler.toml to point to your cf kv store
npx wrangler@latest deploy
```

It is also easiest to use a domain in cloudflare. Set the `*` and `*.*` subdomains to point to the worker you just deployed.