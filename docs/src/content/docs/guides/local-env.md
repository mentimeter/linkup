---
title: Run a Local Linkup Session
description: Get started with linkup by running a local linkup session
---

## Prerequisites

- [Linkup deployed to a Cloudflare domain](/linkup/guides/deploy-linkup)

## Installing the CLI

### With the install script (recommended)

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mentimeter/linkup/refs/heads/main/linkup-cli/install.sh | bash

# Or to install a pre-release version (beta)

curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mentimeter/linkup/refs/heads/main/linkup-cli/install.sh | bash -s -- --channel beta
```

### With Homebrew

```sh
brew tap mentimeter/mentimeter
brew install linkup
```

## Basic workflow

```sh
linkup start              # Start Linkup and get your unique session name
linkup status             # See all running sessions and which services are healthy
linkup route local web    # Route the `web` service to your local dev server
linkup route remote web   # Route the `web` service back to the remote server
linkup stop               # Stop your session and clean up
```

## linkup start

`linkup start` does several things in order:

1. Starts the local server (a reverse proxy that runs in the background on your
   machine)
2. Uploads your session configuration to the Cloudflare worker
3. Starts a Cloudflare tunnel so remote services can reach your local server
4. Prints a table of your session name and domain URLs
5. Prints a table of your session name and domain URLs

Linkup re-uses your session name across restarts, so your URLs stay stable. A
new name is only generated on the very first run.

### Environment files

For each service that has a `directory` field in your config, Linkup looks for
`.env.*.linkup` files in that directory and appends their contents into the
corresponding `.env.*` file (e.g. `.env.development.linkup` →
`.env.development`). Your services use these files to point at your Linkup
domain. The added block is clearly delimited and is reverted by `linkup stop`.

### Start modes

See [Managing Sessions](/linkup/guides/sessions) for a full comparison of
session types.

## linkup route

`linkup route` changes which URL Linkup routes traffic for a named service to:
either the `local` or `remote` URL defined in your config. The change takes
effect immediately, with Linkup pushing the updated state to the local server
(and, for tunneled sessions, on to the Cloudflare worker).

```sh
linkup route local web        # Route `web` to http://localhost:3000 (or whatever local is set to)
linkup route remote web       # Route `web` back to https://web-dev.hosting-provider.com
linkup route local --all      # Switch every service to local at once
linkup route remote --all     # Switch every service to remote at once
```

## linkup status

Shows a session table (all running sessions) and, for the target session, the
live health of every service, checked in parallel as you watch.

```sh
linkup status                     # Inspect the main session
linkup status --session my-feature  # Inspect a specific session
linkup status --json              # Machine-readable output
```

The service table shows each service's name, whether it is currently routing to
`local` or `remote`, and whether its health endpoint is responding.

## linkup stop

Stops the local server and the Cloudflare tunnel. Also reverts the env file
changes that `linkup start` made: the Linkup block is removed from each `.env.*`
file, restoring the files to their original state.

See [Managing Sessions](/linkup/guides/sessions) for details.
