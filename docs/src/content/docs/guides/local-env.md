---
title: Run a Local Linkup Session
description: Get started with linkup by running a local linkup session
sidebar:
  order: 1
---

## Prerequisites

- [Linkup deployed to a Cloudflare domain](/linkup/guides/deploy-linkup)

## Installing the CLI

### With Homebrew

To use link up locally the easiest way to get started is to use the linkup cli:

```sh
brew tap mentimeter/mentimeter
brew install linkup
```

### Using the install.sh script

```sh
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mentimeter/linkup/refs/heads/main/linkup-cli/install.sh | sh

# Or to install a pre-release version (beta)

INSTALL_PRERELEASE=1 curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mentimeter/linkup/refs/heads/main/linkup-cli/install.sh | sh
```

Once you have the cli installed you can start a linkup session by running:

```zsh
linkup start      <--- Gives you your unique session name
linkup status     <--- Shows how your session is configured
linkup local web  <--- Routes traffic of the `web` service to your local machine
linkup stop       <-- Stops your session
```
