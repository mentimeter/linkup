---
title: Troubleshooting
description: How to troubleshoot common issues with Linkup
---

## linkup health

Before digging into specific issues, run `linkup health`. It gives you a full
snapshot of your installation:

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

For tunneled sessions, Linkup uses `cloudflared` to bring up a Cloudflare named
tunnel against the credentials provisioned by your Linkup deployment.

#### Symptoms

The tunnel either fails to start, or it comes up but DNS for its hostname hasn't
propagated:

```
Failed to start: ...
Failed to verify that DNS got propagated
```

#### Diagnosis

`cat ~/.linkup/cloudflared-stderr` gives you the cloudflared process logs.
`linkup health` also reports whether cloudflared is running and on which PID.

#### Solution

- Check your network connection, then run `linkup stop` followed by
  `linkup start` to try again.
- If the problem persists, Cloudflare may be having issues. Check their
  [status page](https://www.cloudflarestatus.com/).
- If you don't need remote services to call back into your machine, switch to an
  isolated session, which skips the tunnel entirely. Isolated sessions require
  [Local DNS](/linkup/guides/local-dns) to be installed.

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

Set the `LINKUP_CONFIG` environment variable to the path of your config file, or
pass it directly:

```sh
linkup start --config /path/to/linkup.yaml
```

Add the following to your `.zshrc` or `.bashrc` to avoid this problem in future:

```sh
export LINKUP_CONFIG=/path/to/linkup.yaml
```

---

### Orphan processes

If Linkup crashes or is killed unexpectedly, background processes (the local
server or cloudflared) can be left running. `linkup health` lists these under
"Possible orphan processes" along with their PIDs. You can kill them manually
with `kill <pid>`, then run `linkup start` again.

---

### 422 "unable to determine the session origin"

Linkup needs to map every incoming request to a session. The worker returns 422
in two situations: it couldn't extract any session name from the request, or it
extracted one but no session by that name exists in the worker's KV.

```
HTTP 422
Linkup was unable to determine the session origin of the request.
Make sure your request includes a valid session ID in the referer or tracestate headers.
```

#### No session name on the request

The worker checks the URL subdomain, `x-forwarded-host`, `Referer`, `Origin`,
and `tracestate` (in that order). If none yields a name, the request is
rejected. Common causes:

- A `curl` or backend-to-backend request that doesn't carry any of those
  headers. Add `-H "Referer: https://<your-session>.<your-linkup-domain>"` or
  `-H "tracestate: linkup-session=<your-session>"` to test.
- A backend service is calling another backend without propagating the W3C trace
  context. Instrument the caller with an OpenTelemetry HTTP library (see
  [Configure Linkup](/linkup/guides/configure#remote-services)).

#### Session name doesn't match any active session

The headers point to a session that the worker doesn't know about. Common
causes:

- The session was cleaned up after 7 days of inactivity (see
  [Managing Sessions](/linkup/guides/sessions#inactive-session-cleanup)). Run
  `linkup start` to recreate it.
- The session was never created in the worker. For example, an isolated session
  referenced by name from outside your machine, or a typo in a hand-crafted
  `Referer`/`tracestate` header.
- The session belongs to a different Linkup deployment than the worker the
  request is hitting.

---

### 404 "no target for the request"

The request was matched to a valid session but no service in your config matches
the requested host or path:

```
HTTP 404
The request belonged to a session, but there was no target for the request.
Check your routing rules in the linkup config for a match.
```

Check your `domains` block in `linkup.yaml`. Either the host the request is
going to isn't listed in `domains`, or the path doesn't match any `routes` entry
and there is no `default_service`.

---

### 422 "session has no associated tunnel"

```
HTTP 422
This linkup session has no associated tunnel / was started with --no-tunnel
```

This means the request reached the public worker for a session that doesn't have
a tunnel: an isolated session that's expected to be served entirely from your
machine.

The fix depends on what you intended:

- If you intended a tunneled session, run `linkup stop` then `linkup start`
  (without `--isolated`) to bring up the tunnel.
- If you intended an isolated session, the request shouldn't be reaching the
  public worker at all. Make sure [Local DNS](/linkup/guides/local-dns) is
  installed so that `{session}.{linkup-domain}` resolves to your machine instead
  of going out to the internet.

---

### 401 "Your Linkup CLI is outdated"

When the CLI talks to the worker (during `linkup start`, `linkup sessions ...`,
etc.), it sends an `x-linkup-version` header. If the worker has been upgraded
and your CLI is too old, you'll see a 401:

```
Your Linkup CLI is outdated, please upgrade to the latest version.
```

Run `linkup update` to fetch the latest binary. Two related messages have the
same fix:

- `Invalid x-linkup-version header.`: the header is present but unparseable.
  Usually fixed by `linkup update`.
- `No x-linkup-version header, please upgrade your Linkup CLI.`: old CLIs that
  pre-date the version header. Same fix.

---

### Linux: cannot bind to ports 80/443

On Linux, the local server needs the `cap_net_bind_service` capability to bind
to privileged ports. `linkup update` and the install script set this for you,
but if you copied the binary manually or the capability got dropped, you'll see
a permission-denied error from the local server.

Re-apply the capability:

```sh
sudo setcap cap_net_bind_service=+ep "$(which linkup)"
```

`linkup health` reports whether the capability is set under the binary's
section.
