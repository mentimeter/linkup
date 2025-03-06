---
title: How does Linkup work?
description: How to configure services to work with Linkup
---

In order to grasp how linkup works, it's important to understand three core concepts in linkup:

1. Linkup manages "sessions" which are unique views of connected services
2. Linkup can route individual requests to very specific components
3. Linkup can run components that enable requests to be routed by linkup

## Linkup Sessions and Subdomains

Whenever you create a new linkup session, either for a local environment or for a preview environment, you will receive a unique linkup session name, which is a subdomain of your linkup domain.

For example, `linkup start` might give you a `slim-gecko.example.com` (local environment) subdomain. Or `linkup preview` might give you `xyz123.example.com` (preview environment) subdomain.

This linkup session name will be used to identify all of the requests that belong to that session.

## Identifying and Routing Requests to Sessions

All requests that reach linkup go through an identification process to determine which session they belong to. Requests that can't be identified are rejected as a precaution.

Request to session identification uses two methods:

- Common browser headers, primarily `Referer`.
- Opentelemetry tracing headers `traceparent` and `tracestate`.

For all requests you make within a linkup session, they will either come straight from the browser (identified by your linkup subdomain `slim-ant.domain.com`), or they will come from an underlying service.

Linkup will add opentelemetry tracing headers to all requests it receives, but you will likely need to _propogate_ these headers through your services. Please refer to the [OpenTelemetry documentation on how to do this for your specific language and framework](https://opentelemetry.io/docs/languages/).


### Routing Requests Example

Let’s study this example linkup session state:

```json
{
  "services": [
    {
      "name": "frontend",
      "location": "https://my-pr-frontend-123.preview.com",
    },
    {
      "name": "api",
      "location": "https://latest-api-main.shared-infra.com",
    },
    {
      "name": "auth",
      "location": "https://latest-auth-main.shared-infra.com",
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

Let's work through a few example requests to this environment. Given that your session name is `smart-snake`, what do you think will happen for the following requests:

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

Generally then, the best way to think about the question "will linkup be able to identify this request?" is to think about whether you can answer yes to either of the following questions:

- Does the request come straight from the browser? Then it will have a `Referer` header that includes the linkup session name.
- Have I instrumented the underlying service to propogate the opentelemetry tracing headers? If so, `tracestate` will include the linkup session name.