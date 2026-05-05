---
title: Create a Preview Environment
description: Connect remote services to a persistent linkup preview environment
---

## Prerequisites

- Linkup deployed to a Cloudflare Domain

## What is a Preview Environment?

`linkup start` is designed to:

- work with services that you run _locally_ (on localhost)
- run with a tunnel that makes your local service available on the internet
- only be used by one engineer (not available when dev laptop is off)

A preview environment, on the other hand:

- consists only of services deployed to the internet (e.g. `my-pr-deploy-123.previewinfra.com`)
- has no tunnels or tunneling infrastructure to `localhost`
- is always online, as long as the underlying services are up

## Creating a preview environment

Use `linkup sessions create-preview` with `<service>=<url>` pairs:

```
linkup sessions create-preview

Create a preview session

Usage: linkup sessions create-preview [OPTIONS] [NAME] [SERVICES]...

Arguments:
  [NAME]         Optional name for the preview session
  [SERVICES]...  <service>=<url> pairs to override

Options:
      --print-request  Print the request body instead of sending it
  -h, --help           Print help
```

For example:

```sh
linkup sessions create-preview web=https://my-pr-deploy-123.example.com
```

You can also give the session a memorable name and override multiple services:

```sh
linkup sessions create-preview my-pr \
  web=https://my-pr-frontend-123.example.com \
  backend=https://my-pr-api-123.example.com
```

## Listing sessions

```sh
linkup sessions list
```

## How it differs from local sessions

| Feature | `linkup start` (tunneled) | `linkup sessions create-preview` |
|---------|--------------------------|----------------------------------|
| Services | Local + remote | Remote only |
| Tunnel required | Yes | No |
| Available when laptop is off | No | Yes |
| Use case | Local development | CI/CD, sharing with teammates |
