---
title: How does Linkup work?
description: How to configure services to work with Linkup
---

To grasp how linkup works, three core concepts matter:

1. Linkup manages "sessions" which are unique views of connected services
2. Linkup can route individual requests to very specific components
3. Linkup can run components that enable requests to be routed by linkup

## Linkup Sessions and Subdomains

Whenever you create a new linkup session, either for a local environment or for
a preview environment, you will receive a unique linkup session name, which is a
subdomain of your linkup domain.

For example, `linkup start` might give you a `slim-gecko.example.com` (local
environment) subdomain. Or `linkup sessions create-preview` might give you
`xyz123.example.com` (preview environment) subdomain.

This linkup session name will be used to identify all of the requests that
belong to that session.

## Identifying and Routing Requests to Sessions

All requests that reach linkup go through an identification process to determine
which session they belong to. Requests that can't be identified are rejected
with a `422` status as a precaution.

Linkup tries the following sources, in order, and uses the first one that yields
a known session name:

1. The first subdomain of the request URL itself (e.g. `slim-ant` from
   `slim-ant.domain.com`).
2. The first subdomain of the `x-forwarded-host` header. The Cloudflare tunnel
   uses this header to carry session identity back to the local server.
3. The first subdomain of the `Referer` header. Browser-tab requests rely on
   this to stay attributed when they go to a non-session host (like
   `api.domain.com`).
4. The first subdomain of the `Origin` header. Acts as a fallback for redirects,
   where the `Referer` may be missing.
5. The session name embedded in the W3C `tracestate` header
   (`linkup-session=<name>`). Backend-to-backend calls rely on this to stay
   attributed.

Linkup adds OpenTelemetry tracing headers to every request it forwards, but for
the chain to work end-to-end your backend services need to _propagate_ those
headers through their outbound HTTP calls. Refer to the
[OpenTelemetry documentation for your language or framework](https://opentelemetry.io/docs/languages/).

### Routing Requests Example

Let’s study this example linkup session state:

```json
{
  "services": [
    {
      "name": "frontend",
      "location": "https://my-pr-frontend-123.preview.com"
    },
    {
      "name": "api",
      "location": "https://latest-api-main.shared-infra.com"
    },
    {
      "name": "auth",
      "location": "https://latest-auth-main.shared-infra.com"
    }
  ],
  "domains": [
    {
      "domain": "example.com",
      "default_service": "frontend",
      "routes": [
        {
          "path": "/auth.*",
          "service": "auth"
        }
      ]
    },
    {
      "domain": "api.example.com",
      "default_service": "api",
      "routes": null
    }
  ]
}
```

Let's work through a few example requests to this environment. Given that your
session name is `smart-snake`, what do you think will happen for the following
requests:

<details>
  <summary><code>curl -I https://smart-snake.example.com</code></summary>
  <p>HTTP <code>200</code>, routed to <code>frontend</code> service</p>
</details>

<details>
  <summary><code>curl -I https://smart-snake.example.com/auth/login</code></summary>
  <p>HTTP <code>200</code>, routed to <code>auth</code> service</p>
</details>

<details>
  <summary><code>curl -I https://api.example.com/</code></summary>
  <p>HTTP <code>422</code>, no way to identify session</p>
</details>

<details>
  <summary><code>curl -I https://api.example.com/ -H "Referer: https://smart-snake.example.com"</code></summary>
  <p>HTTP <code>200</code>, routed to <code>api</code> service</p>
</details>

<details>
  <summary><code>curl -I https://api.example.com/ -H "tracestate: linkup-session=smart-snake"</code></summary>
  <p>HTTP <code>200</code>, routed to <code>api</code> service</p>
</details>

Generally then, the best way to think about the question "will linkup be able to
identify this request?" is to think about whether you can answer yes to either
of the following questions:

- Does the request come straight from the browser? Then it will have a `Referer`
  header that includes the linkup session name.
- Have I instrumented the underlying service to propogate the opentelemetry
  tracing headers? If so, `tracestate` will include the linkup session name.

## Linkup Components

To route these requests based on the identifying information, Linkup needs to
run a few components in different places.

### Linkup Cloudflare Worker

The Linkup Cloudflare worker is configured to intercept all requests that reach
your Cloudflare zone. A Cloudflare zone is approximately equivalent to a domain,
so if you have the domain example.com, it will intercept all requests that are
made to *.example.com. This means is that linkup can function as a
man-in-the-middle proxy between all requests that your application makes, and
can reroute requests to the correct service based on its headers.

### Cloudflare Tunnel & The Local Server

To direct traffic to servers running on `localhost`, the linkup CLI runs a
Cloudflare tunnel paired with a local proxying server. Together they receive
requests made from a remote component and deliver them to a server running on
your local machine.

### Local DNS

In its default mode, Linkup has a fairly strong dependency on the network. For
frontend engineers who are running development servers, they may have pages that
require 50-100 mb of JavaScript to load.

To speed up cases where the network is a bottleneck, Linkup offers an optional
local DNS mode. Local DNS resolves your application's domains directly to
servers running on your local machine, so requests that could have been handled
locally don't go over the public internet. Linkup also manages the TLS
certificates for those local domains so HTTPS works without browser warnings.
