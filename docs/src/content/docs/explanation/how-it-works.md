---
title: How does Linkup work?
description: How to configure services to work with Linkup
---

## Linkup Sessions and Subdomains

Whenever you create a new linkup session, either for a local environment or for a preview environment, you will receive a unique linkup session name, which is a subdomain of your linkup domain.

For example, `linkup start` might give you a `slim-gecko.example.com` (local environment) subdomain. Or `linkup preview` might give you `xyz123.example.com` (preview environment) subdomain.

This linkup session name will be used to identify all of the requests that belong to that session.

## Identifying Requests to Sessions

All requests that reach linkup go through an identification process to determine which session they belong to. Requests that can't be identified are rejected as a precaution.

Request to session identification uses two methods:

- Common browser headers, primarily `Referer`.
- Opentelemetry tracing headers `traceparent` and `tracestate`.

For all requests you make within a linkup session, they will either come straight from the browser (identified by your linkup subdomain `slim-ant.domain.com`), or they will come from an underlying service.

Linkup will add opentelemetry tracing headers to all requests it receives, but you will likely need to _propogate_ these headers through your services. Please refer to the [OpenTelemetry documentation on how to do this for your specific language and framework](https://opentelemetry.io/docs/languages/).

