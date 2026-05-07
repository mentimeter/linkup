---
title: Updating and Uninstalling
description: How to keep Linkup up to date and remove it if needed
---

## Updating

### With Homebrew

If you installed Linkup via Homebrew, update through Homebrew:

```sh
brew upgrade linkup
```

### With linkup update

If you installed Linkup via the install script, use the built-in update command:

```sh
linkup update
```

This stops any running Linkup session, downloads the latest binary for your
platform, swaps it in place, and reports when it's done. On Linux, it also
re-applies the `cap_net_bind_service` capability needed to bind to ports 80/443.

To update to (or stay on) the pre-release channel, pass `--channel beta`:

```sh
linkup update --channel beta
```

To go back to stable:

```sh
linkup update --channel stable
```

The CLI caches the latest known release to avoid hitting the API on every
command. To bypass that cache, pass `--skip-cache`:

```sh
linkup update --skip-cache
```

---

## Uninstalling

```sh
linkup uninstall
```

You will be asked to confirm before anything is removed. On confirmation, it:

1. Stops any running Linkup session (`linkup stop`)
2. Uninstalls Local DNS if it was installed (`linkup local-dns uninstall`)
3. Removes the Linkup binary, using the right method for how you installed it
   (Homebrew, Cargo, or manual script)
4. Removes the `~/.linkup/` directory and all state, certificates, and logs
   stored there
