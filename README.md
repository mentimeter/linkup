# linkup

> Run the services you change, get the rest for free

Linkup lets you combine local and remote services to create cheap yet complete development environments.

Linkup is written in rust, and uses a combination of cloudflare workers and tunnels under the hood.

## How it works

Engineers often need a complete copy of a system to develop on, even though they only change one or two services at a time.

Linkup lets you create many "virtual copies" of a system, each with a different set of services running locally and remotely. We call each unique virtual copy a _linkup session_.

![linkup-routing](./docs/linkup-routing.svg)

For example, Peter here can use a local copy of their web development server, but they can use the remote / shared backend server without having to run anything locally.

Mary's pull request can deploy a preview of their backend that can be accessed through the remote / shared web server.

## Using Linkup

**There is more detailed information about running and debugging linkup sessions in [docs/using-linkup](./docs/using-linkup.md)**

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

## Configuring Linkup

Linkup is configured using a yaml file when you start your linkup session. This file describes the services that make up your system, and how they should be combined into linkup sessions.

Here is an example:

```yaml
linkup:
  remote: https://where.linkup.is.deployed.com
  # By default, linkup will ensure _nothing_ is cached for dev environments
  # to update on save, you can override this behaviour for specific paths
  # by adding them to the cache_routes list
  cache_routes:
    - .*/_next/static/.*
    - .*/_next/data/.*
services:
  - name: web
    remote: https://web-dev.hosting-provider.com
    local: http://localhost:3000
    # Can be used to rewrite request paths
    rewrites:
      - source: /foo/(.*)
        target: /bar/$1
  - name: backend
    remote: https://api-dev.hosting-provider.com
    local: http://localhost:9000
    directory: ./relative/path/to/backend
domains:
  - domain: dev-domain.com
    default_service: web
    routes:
      - path: /api/v1/.*
        service: backend
  - domain: api.dev-domain.com
    default_service: backend
```

### Local environment variables

When booting local servers to run in linkup, they must be configured with environment variables that point to your linkup domain.

To do this, linkup appends the contents of `.env.linkup` to the `.env` file located in the `directory` configuration field of the service.

Linkup will fail to start if there is no `.env.linkup` file, and it will warn you to restart your local server if it was already booted on `linkup start`.

## Deploying Linkup

In order to run linkup sessions, you need:

- A dedicated domain for the linkup cloudflare worker to run on
- Deployed copies of the remote services you want to provide

### Configuring the domain & worker

Linkup is deployed as a cloudflare worker with a key-value store, and can be deployed using the wrangler cli:

```sh
cd worker
cp wrangler.toml.sample wrangler.toml
# Edit wrangler.toml to point to your cf kv store
npx wrangler@latest deploy
```

It is also easiest to use a domain in cloudflare. Set the `*` and `*.*` subdomains to point to the worker you just deployed.

### Deploying remote services

The remote services you would like to make available to linkup sessions have a few requirements:

- They must be accessible at a public url
- Their environment variables must be configured to point to your linkup domain
- For requests _from_ the remote service to be correctly routed to linkup sessions, they must be able to [propagate trace contexts](https://www.w3.org/TR/trace-context/) through requests made from the service. The easiest way to acheive this is to use an OpenTelemetry client library to instrument your http client. Here is [an example for javascript](https://www.npmjs.com/package/@opentelemetry/instrumentation-http).
