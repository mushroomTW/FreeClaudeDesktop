# FreeClaudeDesktop

Requires Bun 1.4.3 or later. Install and manage FreeClaudeDesktop with Bun:

```sh
bun add -g @mushroomtw/freeclaudedesktop
freecd start
```

Use `freecd status`, `freecd restart`, `freecd dashboard`, and `freecd path` to manage the service. The web dashboard is available at `http://127.0.0.1:3000/dashboard` by default.

## Uninstall

A global installation can clean up local state and remove the package in one step:

```sh
freecd uninstall
```

After a successful cleanup, the Bun wrapper waits for the native program to exit before removing the current Bun global package. A failed cleanup keeps the package; if package removal fails, retry `bun remove -g @mushroomtw/freeclaudedesktop --ignore-scripts`. Source builds and local installations only clean up application state. When global ownership cannot be confirmed, the wrapper prints the manual removal command. `freecd purge` keeps the package.

## Memory and performance

The resident runtime runs the Proxy and Companion in a single `freeclaude` process. The Proxy forwards the upstream stream chunk by chunk, and each request body is limited to `16 MiB`; memory usage varies with the number of concurrent requests and streams, so there is no fixed RAM guarantee. Each Tokio runtime uses at most `2` async worker threads by default, adjustable with `TOKIO_WORKER_THREADS`.

## Migrating from npm

First clean up local state with the old program, then remove the old commands through whatever process previously managed the global npm package; finally install with Bun:

```sh
freecd uninstall
bun add -g @mushroomtw/freeclaudedesktop
freecd install
```

Cleanup deletes application data, so you must configure the Gateway again after reinstalling.
