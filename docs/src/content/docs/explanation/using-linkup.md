---
title: Using Linkup
description: How to use Linkup to develop and test your services locally.
---
# Running linkup

Normally, starting linkup looks like this:

```zsh
linkup start      <--- Gives you your unique session name
linkup status     <--- Shows how your session is configured
linkup local dashboard  <--- Routes traffic of the `dashboard` service to your local machine
linkup stop       <-- Stops your session
```

## Speeding up your linkup session with `local-dns`

Your linkup domains live on the public internet. If you're running heavy frontend servers that load 50mb of js assests, you might find that your linkup session feels slow.

Linkup comes with a public-internet-bypass mechanism called `local-dns`. This feature allows you to resolve your linkup domains to your local machine, bypassing the public internet. This will make your frontends serve assets from your local machine, and make your linkup session feel much faster.

To use `local-dns`, run `linkup local-dns install` in your terminal. This will install a local DNS server on your machine that will resolve your linkup domains to your local machine.

### Limitations of `local-dns`

Although much of your traffic will be served from your local machine, some requests will still go through the internet, and therefore still need a functioning tunnel, including:

- When accessing a linkup session from a different device or from a colleague's machine.
- When a remote service needs to access a server on your local machine (a remote frontend making a network request to your local backend).

## Common issues

### Tunnel problems

In order to reach servers started on your local machine, linkup uses a cloudflared tunnel.

#### Symptoms

```
Waiting for tunnel to be ready at https://xxx.trycloudflare.com/...
Error: StartLinkupTimeout("https://xxx.trycloudflare.com/linkup-check took too long to load")
```

#### Diagnosis

`cat ~/.linkup/cloudflared-stderr` will give you more logs from the cloudflared process that might point you in the right direction.
Linkup runs `cloudflared tunnel --url http://localhost:9066` to start the tunnel. You can run this command manually to see if it gives you more information.

#### Solution

- Sometimes, it can be as simple as a network problem. Check your connection and run `linkup reset` to try again.
- If the problem persists, cloudflare may be having problems. Check their [status page](https://www.cloudflarestatus.com/).
- To mitigate the impact of your tunnel being down, you can use `local-dns` to resolve your linkup domains to your local machine.
- With `local-dns` installed, you can run linkup without a tunnel by running `linkup start --no-tunnel`. This will allow you to use your linkup session without a tunnel, but not all use cases will work.

### Configuration problems

Linkup needs a configuration file to start a session.

#### Symptoms

```
➜  ~ linkup start
Error: NoConfig("No config argument provided and LINKUP_CONFIG environment variable not set")
```

#### Diagnosis

`echo $LINKUP_CONFIG` is empty.

#### Solution

- You need to provide a linkup configuration file. You can do this by setting the `LINKUP_CONFIG` environment variable to the path of your configuration file, or by providing the path as an argument to `linkup start`.
- Add a `export LINKUP_CONFIG=/path/to/linkup-config.yaml` to your `.zshrc` or `.bashrc` to avoid this problem in the future.
- If you work at mentimeter, `menti localsecrets` will sort this out for you.
