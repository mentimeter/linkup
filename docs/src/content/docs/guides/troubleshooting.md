---
title: Troubleshooting
description: How to troubleshoot common issues with Linkup
---

## linkup health

Before digging into specific issues, run `linkup health`. It gives you a full snapshot of your installation:

```sh
linkup health
linkup health --json   # machine-readable output
```

It reports:
- System info (OS, architecture)
- Current session name and tunnel URL
- Background service status (local server, cloudflared) with PIDs
- Linkup version and `~/.linkup/` directory contents
- Local DNS installation status
- Any processes that look like orphaned Linkup processes

This is the first thing to share when asking for help with a Linkup issue.

---

## Common issues

### Tunnel problems

In order to reach servers started on your local machine, Linkup uses a cloudflared tunnel.

#### Symptoms

```
Waiting for tunnel to be ready at https://xxx.trycloudflare.com/...
Error: StartLinkupTimeout("https://xxx.trycloudflare.com/linkup-check took too long to load")
```

#### Diagnosis

`cat ~/.linkup/cloudflared-stderr` will give you more logs from the cloudflared process.

Linkup runs `cloudflared tunnel --url http://localhost:9066` internally. You can run this command manually to see if it gives you more information.

#### Solution

- Check your network connection, then run `linkup stop` followed by `linkup start` to try again.
- If the problem persists, Cloudflare may be having issues. Check their [status page](https://www.cloudflarestatus.com/).
- If you don't need remote services to call back into your machine, avoid the tunnel entirely:

  ```sh
  linkup stop
  linkup start --isolated
  ```

- If you do need the tunnel but want to speed up assets in the meantime, install [Local DNS](/linkup/guides/local-dns). It bypasses the tunnel for traffic that can be served locally.

---

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

Set the `LINKUP_CONFIG` environment variable to the path of your config file, or pass it directly:

```sh
linkup start --config /path/to/linkup.yaml
```

Add the following to your `.zshrc` or `.bashrc` to avoid this problem in future:

```sh
export LINKUP_CONFIG=/path/to/linkup.yaml
```

---

### Orphan processes

If Linkup crashes or is killed unexpectedly, background processes (the local server or cloudflared) can be left running. `linkup health` lists these under "Possible orphan processes" along with their PIDs. You can kill them manually with `kill <pid>`, then run `linkup start` again.
