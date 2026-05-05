---
title: Isolated Sessions
description: Run local-only Linkup sessions with no Cloudflare connectivity
---

An isolated session runs entirely on your local machine with no Cloudflare tunnel or worker involved. This is useful when:

- You don't need remote services to call back into your machine
- You want to run multiple independent sessions on the same machine simultaneously
- You're working offline or behind a network that doesn't support tunnels

## How requests reach the session

An isolated session is reachable at the same `{session-name}.{linkup-domain}` URL as a tunneled session, but with no Cloudflare tunnel routing public traffic in. For your browser to reach it, the hostname has to resolve to your local machine, so `linkup local-dns install` is a prerequisite for using isolated sessions.

If you haven't installed local-dns yet, see [Local DNS](/linkup/guides/local-dns).

## Starting as an isolated session

Pass `--isolated` to `linkup start` to make your main session isolated instead of tunneled:

```sh
linkup start --isolated
```

To switch modes, stop first:

```sh
linkup stop
linkup start --isolated   # or just `linkup start` to go back to tunneled
```

## Creating additional isolated sessions

With Linkup already running, you can create extra isolated sessions alongside your main session:

```sh
linkup sessions create-isolated
# or give it a name:
linkup sessions create-isolated my-feature
```

Each isolated session gets its own name and can have services independently routed:

```sh
linkup route local web --session my-feature
linkup route remote backend --session my-feature
```

## Deleting an isolated session

```sh
linkup sessions delete my-feature
```

This removes the session from the local server. To stop the main session entirely, use `linkup stop`.
