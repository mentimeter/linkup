---
title: Required Cloudflare Resources
description: A description of the resources you need to use Linkup with Cloudflare
---

Account-scoped:

- A Cloudflare Worker script running the Linkup worker
- Two Cloudflare KV namespaces: `LINKUP_SESSIONS` (session state) and
  `LINKUP_TUNNELS` (provisioned tunnel records)
- An account-level API token used by the worker to provision tunnels

Per zone you deploy to:

- DNS records (`*` and `@`) pointing the zone at the worker
- Worker routes `*.mydomain.com/*` and `mydomain.com/*` binding the zone's
  traffic to the worker

On the tunnel zone (the first zone passed to `--zone-ids`) only:

- DNS records for `linkup-tunnel-*` hostnames, created at runtime as sessions
  start
- A cache rule that excludes those tunnel hostnames from Cloudflare's cache
