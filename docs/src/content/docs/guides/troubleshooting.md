---
title: Troubleshooting
description: How to troubleshoot common issues with Linkup
---

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
