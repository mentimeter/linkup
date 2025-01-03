---
title: Run a Local Linkup Session 
description: Get started with linkup by running a local linkup session
sidebar:
  order: 1
---

## Prerequisites

- [Linkup deployed to a Cloudflare domain](/linkup/guides/deploy-linkup)

## Installing the CLI

To use link up locally the easiest way to get started is to use the linkup cli:

```sh
brew tap mentimeter/mentimeter
brew install linkup
```

Once you have the cli installed you can start a linkup session by running:

```zsh
linkup start      <--- Gives you your unique session name
linkup status     <--- Shows how your session is configured
linkup local web  <--- Routes traffic of the `web` service to your local machine
linkup stop       <-- Stops your session
```
