---
title: Using Local DNS
description: How Linkup's local DNS works and when it's needed
---

`linkup local-dns` makes your Linkup hostnames (e.g. `slim-gecko.example.com`)
resolve directly to the local server on your machine, bypassing public DNS and
Cloudflare. It's a one-time setup per machine. Whether it's optional or required
depends on the session type. See [When you need it](#when-you-need-it).

## How it works

`linkup local-dns install` does three things, all requiring `sudo`:

1. Writes `/etc/resolver/<domain>` files for each of your Linkup domains,
   pointing at `127.0.0.1:8053`. macOS (and Linux with a compatible resolver)
   consult these files before public DNS, so any subdomain of your Linkup
   domains resolves to your machine.
2. Generates a self-signed certificate authority and adds it to your system
   keychain. The local server then signs TLS certificates for your Linkup
   domains on the fly, so HTTPS works without browser warnings.
3. Flushes the DNS cache so the new resolver config takes effect immediately.

At runtime, the local server listens on `127.0.0.1:8053` for DNS queries and
answers any `*.{linkup-domain}` query with `127.0.0.1`. It then accepts the
HTTPS connection that follows, terminates TLS using a certificate signed by the
installed CA, and forwards the request to whichever URL each service is
currently routed to.

`linkup stop` is invoked automatically during install and uninstall, so Linkup
restarts cleanly with the new configuration.

## When you need it

| Session type                                   | Local DNS                                                                                                                                                                                                                                  |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **Tunneled** (`linkup start`)                  | Optional. Without it, requests still work. They just go out to Cloudflare and back through the tunnel. With it, requests originating on your machine resolve to the local server directly, which is much faster for asset-heavy frontends. |
| **Isolated** (`linkup start --isolated`)       | **Required.** Isolated sessions have no Cloudflare tunnel and no worker involvement, so without local-dns there is no path for your browser to reach `{session}.{linkup-domain}`.                                                          |
| **Preview** (`linkup sessions create-preview`) | Not applicable. Preview sessions consist of remote services only, so there's no local component for local-dns to point at.                                                                                                                 |

Local-dns only affects requests originating on your own machine. Requests from
another device (a colleague's browser, or a deployed service calling back to
you) don't see your `/etc/resolver/` files, so for those you still need a
Cloudflare tunnel.

## Installing

```sh
linkup local-dns install
```

## Uninstalling

```sh
linkup local-dns uninstall
```

This removes the resolver files, removes the CA certificate from your keychain,
and flushes DNS cache.
