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

For tunneled sessions, Linkup uses `cloudflared` to bring up a Cloudflare named tunnel against the credentials provisioned by your Linkup deployment.

#### Symptoms

The tunnel either fails to start, or it comes up but DNS for its hostname hasn't propagated:

```
Failed to start: ...
Failed to verify that DNS got propagated
```

#### Diagnosis

`cat ~/.linkup/cloudflared-stderr` gives you the cloudflared process logs. `linkup health` also reports whether cloudflared is running and on which PID.

#### Solution

- Check your network connection, then run `linkup stop` followed by `linkup start` to try again.
- If the problem persists, Cloudflare may be having issues. Check their [status page](https://www.cloudflarestatus.com/).
- If you don't need remote services to call back into your machine, switch to an isolated session, which skips the tunnel entirely. Isolated sessions require [Local DNS](/linkup/guides/local-dns) to be installed.

  ```sh
  linkup stop
  linkup start --isolated
  ```

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
