---
title: What a Linkup setup looks like?
description: What it could look like to run a full linkup deployment
sidebar:
  order: 2
---

We established in the last chapter that Linkup helps you to connect *shared unchanged services* with services that *have changes*.

We will now explain what a setup might look like that deploys both of these kinds of services and uses a linkup deployment to connect them together.

### Deploying the unchanged services

The best way to keep a set of “unchanged” services available for linkup sessions to use is to build in the deployment of these dev environment copies to your existing CI/CD pipeline.

For example, on deployment to main, you might have a GitHub Actions workflow that redeploys your `frontend-dev` service, so that the `frontend-dev` service used by all other linkup sessions is the latest, most up-to-date version.

Importantly, if your `frontend-dev` service lives on a fixed domain, then all other linkup sessions using that unchanged service won't need to recreate their linkup sessions for the new `frontend-dev` changes from latest main to become live in their sessions.

### Deploying the linkup infrastructure

In order to run complete linkup environments, you will need to deploy a few pieces of linkup infrastructure to a dedicated Cloudflare zone. A Cloudflare zone is approximately equivalent to a domain.

Linkup needs to deploy:

- a Cloudflare worker that absorbs all traffic on your Cloudflare zone
- three Cloudflare key-value stores to handle storing sessions, certificates, and tunnels
- a set of other smaller Cloudflare resources, such as caching rules for tunnels, an API token with access to modify tunnels, and a few routing rules

Linkup comes with a `linkup deploy` command that can deploy these resources to a zone of your choice on your behalf, and a `linkup destroy` command to clean up those resources if needed.

### Connecting the changed services

Linkup allows you to connect either remotely deployed “preview-type” services to linkup sessions, or to connect services that you might have running locally on localhost, like a dev server.

An example of a remotely deployed “preview” might be a deployed a copy of the `frontend-dev` service. For example, you might want to configure a GitHub action on pull requests when the frontend code has been changed to deploy a copy of the `frontend-dev` service to your infrastructure. Then, once the front-end component has been deployed, you can create a linkup session using the `linkup preview` command.

On the other hand, if you have a locally running service on localhost, simply starting the service and telling linkup that you want it connected by running `linkup start` and `linkup local frontend` would be enough for you to get your linkup session running.
