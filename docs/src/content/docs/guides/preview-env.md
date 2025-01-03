---
title: Create a Preview Environment
description: Connect remote services to a persistent linkup preview environment
---

### Prerequisites

- Linkup deployed to a Cloudflare Domain

### What is a Preview Environment?

`linkup start` is designed to:

- work with services that you run _locally_ (on localhost)
- run with a tunnel that makes your local service available on the internet
- only be used by one engineer (not available when dev laptop is off)

A preview environment on the other hand:

- consists only of services deployed to the internet (eg. `my-deploy-pr-123.previewinfra.com`)
- has no tunnels or tunneling infrastructure to `localhost`
- always online / works as long as the underlying services do

### Creating a preview environment

```
linkup preview

Create a "permanent" Linkup preview

Usage: linkup preview [OPTIONS] <SERVICES>...

Arguments:
  <SERVICES>...  <service>=<url> pairs to preview.
```

For example:

```
linkup preview web=https://my-preview-deploy-123.example.com
```