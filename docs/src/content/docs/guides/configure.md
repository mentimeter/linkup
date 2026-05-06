---
title: Configuring Linkup
description: Using a linkup configuration file to describe the layout of your services
---

Linkup is configured using a YAML file. This file describes the services that make up your system and how requests should be routed between them. For a field-by-field schema, see the [Config Reference](/linkup/reference/config).

## Example

```yaml
linkup:
  worker_url: https://where.linkup.is.deployed.com
  worker_token: worker_token_from_linkup_deploy
  # By default, linkup will ensure _nothing_ is cached for dev environments
  # to update on save, you can override this behaviour for specific paths
  # by adding them to the cache_routes list
  cache_routes:
    - .*/_next/static/.*
    - .*/_next/data/.*
services:
  - name: web
    remote: https://web-dev.hosting-provider.com
    local: http://localhost:3000
    # Can be used to rewrite request paths
    rewrites:
      - source: /foo/(.*)
        target: /bar/$1
  - name: backend
    remote: https://api-dev.hosting-provider.com
    local: http://localhost:9000
    directory: ./relative/path/to/backend
domains:
  - domain: dev-domain.com
    default_service: web
    routes:
      - path: /api/v1/.*
        service: backend
  - domain: api.dev-domain.com
    default_service: backend
```

## Pointing Linkup at your config

Set the `LINKUP_CONFIG` environment variable to the path of your config file, or pass it with `--config` each time:

```sh
export LINKUP_CONFIG=/path/to/linkup.yaml
linkup start

# or one-off:
linkup start --config /path/to/linkup.yaml
```

## Environment variables for local services

Services with a `directory` field get their environment variables managed automatically by Linkup. Place one or more `.env.*.linkup` files in that directory. When you run `linkup start`, Linkup reads each one and appends its contents into the matching `.env.*` file (e.g. `.env.development.linkup` → `.env.development`). The injected block is clearly delimited and is removed when you run `linkup stop`.

This is the mechanism by which your locally running services are told to use your Linkup domain URLs (e.g. `API_URL=https://api.dev-domain.com`) instead of hardcoded values.

## Service health checks

Each service can optionally declare a health check that `linkup status` uses to verify it is responding:

```yaml
services:
  - name: backend
    remote: https://api-dev.hosting-provider.com
    local: http://localhost:9000
    health:
      path: /healthz        # path to probe (optional, defaults to /)
      statuses: [200, 204]  # acceptable HTTP status codes (optional, defaults to any 2xx)
```

## Remote services

Remote services (the `remote` URL for each service) need to meet a few requirements to work correctly with Linkup:

- They must be reachable at a stable public URL.
- Their environment variables must point to your Linkup domain (not localhost).
- For requests *from* the remote service to be correctly attributed to a Linkup session, the service must [propagate the W3C trace context](https://www.w3.org/TR/trace-context/) headers through its outbound HTTP calls. The easiest way to achieve this is to use an OpenTelemetry HTTP instrumentation library. [Here is an example for Node.js](https://www.npmjs.com/package/@opentelemetry/instrumentation-http).
