---
title: Shell Completion
description: Generate shell autocompletions for the linkup CLI
---

The `linkup completion` command prints a shell-completion script to standard output. Pipe it to the right place for your shell and tab-completion will work for Linkup invocations.

## Supported shells

`bash`, `zsh`, `fish`, `elvish`, `powershell`.

## Bash

```sh
linkup completion --shell bash > /usr/local/etc/bash_completion.d/linkup
```

Or, to load it inline in a session:

```sh
source <(linkup completion --shell bash)
```

## Zsh

```sh
linkup completion --shell zsh > "${fpath[1]}/_linkup"
```

Reload your shell or run `compinit` for the completion to take effect.

To load it inline in a session:

```sh
source <(linkup completion --shell zsh)
```

## Fish

```sh
linkup completion --shell fish > ~/.config/fish/completions/linkup.fish
```

To load it inline in a session:

```fish
linkup completion --shell fish | source
```
