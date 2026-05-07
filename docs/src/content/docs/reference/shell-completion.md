---
title: Shell Completion
description: Generate shell autocompletions for the linkup CLI
---

`linkup completion` prints a shell-completion script to standard output,
auto-detecting your shell from `$SHELL`. Pipe it to the right place for your
shell and tab-completion will work for Linkup invocations.

To generate completions for a different shell (for example when packaging), pass
`--shell <bash|zsh|fish|elvish|powershell>` explicitly.

## Supported shells

`bash`, `zsh`, `fish`, `elvish`, `powershell`.

## Bash

```sh
linkup completion > /usr/local/etc/bash_completion.d/linkup
```

Or, to load it inline in a session:

```sh
source <(linkup completion)
```

## Zsh

```sh
linkup completion > "${fpath[1]}/_linkup"
```

Reload your shell or run `compinit` for the completion to take effect.

To load it inline in a session:

```sh
source <(linkup completion)
```

## Fish

```sh
linkup completion > ~/.config/fish/completions/linkup.fish
```

To load it inline in a session:

```fish
linkup completion | source
```
