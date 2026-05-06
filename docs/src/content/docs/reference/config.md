---
title: Config Reference
description: Every field accepted by the linkup configuration file
---

A Linkup configuration file is YAML with three top-level keys: `linkup`,
`services`, and `domains`. This page documents every field. For a worked example
and conceptual overview, see [Configure Linkup](/linkup/guides/configure).

## Top-level shape

```yaml
linkup:
  worker_url: <url>
  worker_token: <string>
  cache_routes: [<regex>, ...] # optional

services:
  - name: <string>
    remote: <url>
    local: <url>
    directory: <string> # optional
    rewrites: [<rewrite>, ...] # optional
    health: <health> # optional

domains:
  - domain: <string>
    default_service: <string>
    routes: [<route>, ...] # optional
```

## `linkup`

Global settings for the deployment your CLI talks to.

### `linkup.worker_url`

URL of the deployed Linkup Cloudflare worker. `linkup infra deploy` prints this
value when it finishes.

### `linkup.worker_token`

Token used by the CLI to authenticate against the worker. Same provenance as
`worker_url`.

### `linkup.cache_routes`

Optional array of regex patterns. By default, the worker forces
`Cache-Control: no-cache` on every response so dev environments always reflect
the latest backend state. Any path matching one of the regexes here is exempt
and uses normal cache headers instead. Typical use is content-hashed bundler
output.

```yaml
linkup:
  cache_routes:
    - .*/_next/static/.*
    - .*/_next/data/.*
```

Patterns match if found anywhere in the path. You don't need to anchor with
`.*`.

## `services[]`

Each entry describes one routable service.

### `services[].name`

Identifier referenced by `domains[].default_service`,
`domains[].routes[].service`, and `linkup route <local|remote> <name>`. Must be
unique across services.

### `services[].remote`

URL of the deployed copy of the service. Used when the service is routed to
`remote`.

### `services[].local`

URL of the local copy of the service (typically a `localhost` URL). Used when
the service is routed to `local`.

### `services[].directory`

Optional path (relative to your project) where Linkup looks for `.env.*.linkup`
files when you run `linkup start`. The contents are appended into the matching
`.env.*` file (e.g. `.env.development.linkup` → `.env.development`) and removed
by `linkup stop`. See
[Environment variables for local services](/linkup/guides/configure#environment-variables-for-local-services).

### `services[].rewrites[]`

Optional array of regex-based path rewrites applied to requests routed through
the service. Each entry has:

- `source`: regex matched against the request path
- `target`: replacement string. Standard regex backreferences (`$1`, `$2`, ...)
  refer to capture groups in `source`.

```yaml
services:
  - name: web
    remote: https://web-dev.hosting-provider.com
    local: http://localhost:3000
    rewrites:
      - source: ^/old-prefix/(.*)
        target: /new-prefix/$1
```

Rewrites apply to whichever URL the service is currently routed to (local or
remote) and are evaluated in order.

### `services[].health`

Optional health-check configuration used by `linkup status` to mark the service
healthy or not.

- `path`: path to probe. Defaults to `/`.
- `statuses`: array of HTTP status codes that count as healthy. Defaults to any
  2xx response.

```yaml
services:
  - name: backend
    remote: https://api-dev.hosting-provider.com
    local: http://localhost:9000
    health:
      path: /healthz
      statuses: [200, 204]
```

## `domains[]`

Each entry describes a public hostname that the worker will receive requests on,
and how to map paths under it to services.

### `domains[].domain`

The bare hostname (no scheme, no session subdomain), e.g. `example.com` or
`api.example.com`. The worker matches incoming requests against these, after
stripping the session subdomain.

### `domains[].default_service`

Name of the service that handles requests to this domain when no `routes` entry
matches (or when `routes` is omitted entirely).

### `domains[].routes[]`

Optional array of path-based routing rules. Each entry has:

- `path`: regex matched against the request path
- `service`: name of the service to route to when `path` matches

Routes are evaluated in order, and the first match wins. If none match,
`default_service` is used.

```yaml
domains:
  - domain: example.com
    default_service: web
    routes:
      - path: ^/api/v1/.*
        service: backend
      - path: ^/auth/.*
        service: auth
```
