---
title: Managing Sessions
description: Understand session types and how to manage them with the Linkup CLI
---

A Linkup _session_ is a unique view of your services: a name like `slim-gecko`
plus a configuration that says, for each service, whether traffic should go to a
copy running on your machine or to a deployed URL. Linkup supports two session
types, each backed by different infrastructure and suited to a different
workflow.

## Session Types

|                                      | **Tunneled**                                                        | **Preview**                       |
| ------------------------------------ | ------------------------------------------------------------------- | --------------------------------- |
| **Command to create**                | `linkup start`                                                      | `linkup sessions create-preview`  |
| **Needs the local server**           | Yes                                                                 | No                                |
| **Needs a Cloudflare tunnel**        | Yes                                                                 | No                                |
| **Needs `local-dns`**                | Optional (faster local requests)                                    | N/A                               |
| **Reachable from other machines**    | Yes                                                                 | Yes                               |
| **Stays up when your laptop sleeps** | No                                                                  | Yes                               |
| **Typical use case**                 | Local dev where remote services need to call back into your machine | Sharing a PR deploy, CI/CD checks |

## Tunneled Sessions

The default mode. `linkup start` does three things:

1. Starts a local server on your machine
2. Establishes a Cloudflare tunnel from the worker back to that local server
3. Uploads session state to the Cloudflare worker

When a request hits `slim-gecko.example.com`, the worker intercepts it and looks
up the session. Services routed to `local` are forwarded through the tunnel to
your machine. Services routed to `remote` are forwarded to their deployed URL.
Because routing happens in the public worker, anyone on the internet who knows
your session URL can reach your services. A deployed backend can therefore call
back into your local frontend, but it also means your laptop sits on the public
path.

```sh
linkup start
linkup route local web  # Route `web` traffic to your local dev server
linkup status           # Check which services are where
```

[Local DNS](/linkup/guides/local-dns) is optional here. Installing it makes
requests that originate on your machine skip Cloudflare and resolve directly to
the local server, which is a significant speedup for asset-heavy frontends.

## Preview Sessions

A preview session is composed entirely of remotely deployed services, typically
a per-PR deploy of one service combined with shared deployed copies of
everything else. The session lives in Cloudflare with no local server or tunnel
involved, so it stays up as long as the underlying services do. This is the
session type to use for sharing a build with teammates or for CI/CD checks.

See [Preview Environments](/linkup/guides/preview-env) for a full guide.

## Listing Sessions

```sh
linkup sessions list
# alias:
linkup sessions ls

# JSON output for scripting:
linkup sessions list --json
```

## Routing Traffic

```sh
# Route `web` to local:
linkup route local web

# Route all services to remote:
linkup route remote --all
```

## Inspecting a Specific Session

`linkup status` shows all running sessions by default. To inspect services for a
particular session:

```sh
linkup status --session my-feature
```

## Inactive session cleanup

The Linkup worker runs a scheduled job that deletes any tunnel that has not been
started for 7 days. If you don't run `linkup start` for a week, the next run may
give you a freshly-provisioned tunnel for the same session name. Sessions you
actively use are not affected.

This only applies to tunneled sessions. Preview sessions live in the worker
independently and are not subject to this cleanup.
