# Architecture

This document describes the internal structure and execution paths of FreeClaudeDesktop. For installation, operation, supported platforms, and security guidance, see [README.md](README.md).

## Crate boundaries

| Crate | Internal responsibility |
| --- | --- |
| `free-claude-core` (`core/`) | Shared schemas, persistent settings, model discovery and routing, request/response conversion, and launcher integration. |
| `freeclaude-proxy` (`proxy/`) | Axum routes, authentication, gateway forwarding, SSE conversion, Web Admin, and companion WebSocket handling. |
| `freeclaude` (`cli/`) | Commands that coordinate installation, process lifecycle, profile configuration, autostart, update, and removal. |

Dependencies flow inward: the CLI and proxy depend on `free-claude-core`; the core crate is independent of the HTTP server and command-line interface.

```mermaid
flowchart BT
    CORE["free-claude-core"]
    PROXY["freeclaude-proxy"] --> CORE
    CLI["freeclaude CLI"] --> CORE
    CLI --> PROXY
```

## Runtime topology

```mermaid
flowchart LR
    CD["Claude Desktop"] -->|"Anthropic Messages API"| PX["Proxy"]
    ADMIN["Web Admin"] --> PX
    CLI["CLI / Companion"] --> PX
    PX -->|"OpenAI-compatible Chat Completions"| GW["Configured gateway"]
    GW --> PROVIDER["Model provider"]
    PROVIDER --> GW --> PX --> CD
```

The proxy is the protocol boundary. It accepts Claude-compatible requests, while the gateway adapter sends OpenAI-compatible requests upstream. The companion connection lets the Web Admin coordinate with the host-side CLI process without putting host-control logic inside the proxy container.

## Message execution flow

```mermaid
flowchart TD
    A["Incoming POST /v1/messages"] --> B["Authenticate request and load settings"]
    B --> C{"Local optimization applies?"}
    C -->|"Yes"| D["Generate local response"]
    C -->|"No"| E["Resolve Claude-facing alias"]
    E --> F["Convert Anthropic Messages payload"]
    F --> G["Build upstream request and credentials"]
    G --> H["Forward to gateway"]
    H --> I{"Response stream?"}
    I -->|"No"| J["Convert complete OpenAI response"]
    I -->|"Yes"| K["Convert upstream SSE events"]
    J --> L["Return Anthropic JSON"]
    K --> M["Replay thinking and return Anthropic SSE"]
```

## Model discovery and routing

```mermaid
flowchart TD
    A["GET /v1/models"] --> B["Fetch or read cached upstream model list"]
    B --> C["Normalize model metadata"]
    C --> D["Apply per-model capability overrides"]
    D --> E{"Reasoning capability"}
    E -->|"No"| H["Haiku alias"]
    E -->|"Yes"| F{"Max reasoning supported"}
    F -->|"Yes"| G["Opus alias"]
    F -->|"No"| S["Sonnet alias"]
    G --> R["Publish alias and route cache"]
    S --> R
    H --> R
```

An alias is stable for the returned model-list position and uses a bracketed index, such as `claude-opus-5[0]`. A 1M-context capability is represented by `supports1m: true`; the model ID and display label remain clean. `prefer1m: true` makes the 1M variant the default picker selection when that entry is first, and has no effect without `supports1m`. Alias selection is determined by reasoning support: `max` maps to Opus, other supported reasoning levels map to Sonnet, and models without reasoning support map to Haiku.

During request conversion, Claude `thinking.budget_tokens` is translated to a supported `reasoning_effort` level before the upstream request is sent. In response conversion, upstream reasoning is represented either as native Claude thinking blocks or as inline `<antThinking>` text, according to `reasoning_replay_mode`.

## State ownership

```mermaid
flowchart LR
    SETTINGS["Settings store"] --> PROXY["Proxy request handlers"]
    SETTINGS --> ADMIN["Web Admin settings API"]
    KEYRING["OS keyring"] --> PROXY
    PROFILE["Isolated Claude Desktop profile"] <-->|"configure / sync"| CLI["CLI"]
    CLI --> SETTINGS
    PROXY --> CACHE["Model-route cache"]
```

- The settings store owns non-secret proxy configuration and model capability overrides.
- The operating-system keyring owns gateway credentials; settings APIs do not return them.
- The CLI owns lifecycle and profile synchronization. The proxy owns request-time model cache state.
