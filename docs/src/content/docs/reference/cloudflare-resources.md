---
title: Required Cloudflare Resources
description: A description of the resources you need to use Linkup with Cloudflare
---

- A Cloudflare Worker running the Linkup worker
- A Cloudflare KV store to store Linkup session data
- A domain with CNAME's `*` and `@` pointing to the worker
- Cloudflare worker routes `*.mydomain.com/*` and `mydomain.com/*` pointing to the worker
- A Cloudflare cache rule that excludes tunnel subdomains from being cached
