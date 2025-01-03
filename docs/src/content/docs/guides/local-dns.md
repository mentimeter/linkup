---
title: Using Local DNS
description: Use local DNS to speed up local linkup environments
---

## Speeding up your linkup session with `local-dns`

Your linkup domains live on the public internet. If you're running heavy frontend servers that load 50mb of js assests, you might find that your linkup session feels slow.

Linkup comes with a public-internet-bypass mechanism called `local-dns`. This feature allows you to resolve your linkup domains to your local machine, bypassing the public internet. This will make your frontends serve assets from your local machine, and make your linkup session feel much faster.

To use `local-dns`, run `linkup local-dns install` in your terminal. This will install a local DNS server on your machine that will resolve your linkup domains to your local machine.

### Limitations of `local-dns`

Although much of your traffic will be served from your local machine, some requests will still go through the internet, and therefore still need a functioning tunnel, including:

- When accessing a linkup session from a different device or from a colleague's machine.
- When a remote service needs to access a server on your local machine (a remote frontend making a network request to your local backend).