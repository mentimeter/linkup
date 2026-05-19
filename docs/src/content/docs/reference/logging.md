---
title: Logging
description: How Linkup emits logs and how to control verbosity
---

Linkup logs from two places: the CLI process you invoke (e.g. `linkup start`)
and the long-running local server it spawns in the background. During
`linkup start`, the CLI tails the local server's log file and re-emits each line
onto its own stderr, so you see both streams interleaved in the same
`env_logger` format.

## Controlling the log level

Both the CLI and the local server read the `LINKUP_LOG` environment variable.
The value uses the
[`env_logger` filter syntax](https://docs.rs/env_logger/latest/env_logger/#enabling-logging):
a default level optionally followed by per-module overrides.

```sh
LINKUP_LOG=info linkup start                            # default
LINKUP_LOG=debug linkup start                           # everything at debug
LINKUP_LOG=info,linkup_local_server=debug linkup start  # mixed
```

If `LINKUP_LOG` is unset, the default is `info`.

A handful of noisy crates (`hickory_server`, `hyper_util`, `h2`, `tower_http`)
are pinned to lower levels on top of whatever you set, so a plain
`LINKUP_LOG=debug` doesn't flood the terminal with library internals.

## Persisted log files

The local server's stdout and stderr are also written to files under
`~/.linkup/`, so you can inspect them after the fact:

- `~/.linkup/localserver-stderr` — the server's log output (the file
  `linkup start` tails).
- `~/.linkup/localserver-stdout` — usually empty; captures anything the server
  writes outside of `log::*`.
- `~/.linkup/cloudflared-stderr` — the tunnel process logs.

These files are truncated each time the corresponding background process is
restarted.

## Gotcha: the server's level is fixed at spawn time

The local server reads `LINKUP_LOG` only when it is first started. Once it is
running, subsequent `linkup start` invocations reuse the existing process and do
not re-apply the env var to it. The CLI process you invoke will honor the new
`LINKUP_LOG`, but the lines tailed from the already-running server continue at
the level it was spawned with.

For example:

```sh
LINKUP_LOG=debug linkup start   # server spawned at debug
linkup start                    # CLI runs at info, server stays at debug
```

To change the server's level, stop it first:

```sh
linkup stop
LINKUP_LOG=debug linkup start   # server spawned at the new level
```
