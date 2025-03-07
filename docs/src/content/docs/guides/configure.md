---
title: Configuring Linkup
description: Using a linkup configuration file to describe the layout of your services
---

## Configuring Linkup

Linkup is configured using a yaml file when you start your linkup session. This file describes the services that make up your system, and how they should be combined into linkup sessions.

Here is an example:

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

### Local environment variables

When booting local servers to run in linkup, they must be configured with environment variables that point to your linkup domain.

To do this, linkup appends the contents of `.env.linkup` to the `.env` file located in the `directory` configuration field of the service.

### Deploying remote services

The remote services you would like to make available to linkup sessions have a few requirements:

- They must be accessible at a public url
- Their environment variables must be configured to point to your linkup domain
- For requests _from_ the remote service to be correctly routed to linkup sessions, they must be able to [propagate trace contexts](https://www.w3.org/TR/trace-context/) through requests made from the service. The easiest way to acheive this is to use an OpenTelemetry client library to instrument your http client. Here is [an example for javascript](https://www.npmjs.com/package/@opentelemetry/instrumentation-http).