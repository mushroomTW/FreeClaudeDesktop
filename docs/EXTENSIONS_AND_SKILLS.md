# Extensions & Local Optimizations

FreeClaudeDesktop implements a set of **local optimization handlers** that intercept known Claude Desktop / Claude Code auxiliary requests *before* they reach the gateway. Each handler matches a deterministic request pattern and returns an immediate synthetic response, saving latency, tokens, and gateway quota.

> Execution order (cheapest / most common first) — `core/src/optimization/mod.rs:try_optimizations`:
>
> 1. Quota Check Mock → 2. Prefix Detection → 3. Title Skip → 4. Suggestion Skip → 5. Filepath Extraction → 6. Safety Classifier Skip → 7. Web Tools

All toggles live in `core/src/core/config.rs:OptimizationSettings` and are exposed in Dashboard → Optimizations. Defaults are `true` except `enable_web_server_tools` and `enable_api_call_logging` (`false`).

| Toggle | Field | Default |
| --- | --- | --- |
| Quota Check Mock | `enable_quota_check_mock` | `true` |
| Prefix Detection | `enable_prefix_detection` | `true` |
| Skip Title Generation | `enable_title_generation_skip` | `true` |
| Skip Suggestion Mode | `enable_suggestion_mode_skip` | `true` |
| Filepath Extraction Mock | `enable_filepath_extraction_mock` | `true` |
| Skip Safety Classifier | `enable_safety_check_skip` | `true` |
| Web Tool Intercept | `enable_web_server_tools` | `false` |
| API Call Logging | `enable_api_call_logging` | `false` |

---

## 1. Quota Check Mock (`quota_check_mock`)

**What it does:** Answers Claude Desktop's background quota / liveness probes locally without calling the gateway. Returns `"配額檢查通過。"` (quota check passed).

**Detection** — `core/src/optimization/detection.rs:is_quota_check_request`:

* `max_tokens == 1` and exactly one message, **and** either:
  * **Legacy probe:** `role == "user"`, `tools` is non-empty, and content (case-insensitive, trimmed) is exactly `"count"`; *or*
  * **Modern background probe:** non-streaming (`stream != true`), no `tools`, no `system`, and raw body length ≥ 512 bytes (constant `BACKGROUND_PROBE_MIN_BODY_BYTES`). The single message role is deliberately ignored — newer Claude Desktop uses non-`user` roles for this probe.

This does **not** match ordinary short questions like `"What is my quota?"` — covered by `ordinary_short_quota_question_is_not_a_probe` test.

**Response:** `core/src/optimization/mod.rs:build_optimized_text_response` with 10 input / 5 output tokens, JSON or SSE depending on `stream`.

**Benefit:** Skips the most frequent background health-check, sub-millisecond locally.

---

## 2. Command Prefix Quick Detection (`prefix_detection`)

**What it does:** Resolves shell-command intent locally instead of asking the LLM.

**Detection** — `core/src/optimization/detection.rs:extract_command_prefix`:

* Single message, content contains both `"<policy_spec>"` and `"Command:"`.
* The substring after `"Command:"` is fed to `core/src/optimization/command_utils.rs:parse_shell_command_prefix`.

**Prefix logic — `command_utils::parse_shell_command_prefix`:**

* Trims input, strips leading env assignments (`ENV=prod npm install` → `npm install`).
* Blocks command injection: returns `"command_injection_detected"` if input contains `` ` `` or `$(`.
* Two-word commands are preserved for `git`, `npm`, `docker`, `kubectl`, `cargo`, `go`, `pip`, `yarn` — e.g. `"git commit -m 'hi'"` → `"git commit"`, `"docker build -t app ."` → `"docker build"`. If the second token is a flag (`-p`), only the first word is returned.

**Response:** The extracted prefix string (e.g. `"git commit"`) with 100 input / 5 output tokens.

---

## 3. Skip Title Generation (`title_generation_skip`)

**What it does:** Intercepts the hidden "generate a short title for this chat" request that Claude Desktop fires after the first user message, and returns a fixed title `"Conversation"`.

**Detection** — `core/src/optimization/detection.rs:is_title_generation_request`:

* `system` exists, `tools` absent.
* `system` text is extracted via `extract_system_text` which supports both `String` and `Array` (`[{type:"text",text:...}]`) formats — required for Claude Code's array-style system prompts.
* Lowercased system text must contain `"title"` and one of:
  * `"sentence-case title"` or `"<title>"`, or
  * `"generate a short title"` / `"concise title"` / `"suggest a title"`, or
  * (`"return json"` + `"field"` + (`"coding session"` or `"this session"`)).

**Response:** `"Conversation"` with 100 input / 5 output tokens.

---

## 4. Skip Suggestion Mode (`suggestion_mode_skip`)

**What it does:** Drops the "suggest follow-up questions" auxiliary call.

**Detection** — `core/src/optimization/detection.rs:is_suggestion_mode_request`:

* Any `user` message whose `content` (string) contains `"[SUGGESTION MODE:"`.

**Response:** Empty string `""` with 100 input / 1 output token.

---

## 5. Filepath Extraction Mock (`filepath_extraction_mock`)

**What it does:** Extracts file paths from a command + its output locally, without sending large logs to the LLM.

**Detection** — `core/src/optimization/detection.rs:extract_filepaths`:

* Single message, no `tools`, content contains both `"Command:"` and `"Output:"`.
* Either the user content contains `"filepaths"` (case-insensitive) or the system text contains `"extract any file paths"` or `"file paths that this command"`.
* Parses `Command:` and `Output:` sections, then calls `extract_filepaths_from_command`.

**Extraction — `detection::extract_filepaths_from_command`:**

* `ls`, `dir`, `find`, `tree`, `pwd`, `cd`, `mkdir`, `rmdir`, `rm` → `<filepaths>\n</filepaths>` (no files).
* `cat`, `head`, `tail`, `less`, `more`, `bat`, `type` → positional args excluding flags.
* `grep` → positional file args with proper flag handling (`-e`/`-f`/`-m`/`-A`/`-B`/`-C`, pattern vs. file disambiguation).
* All other commands → empty filepaths block.

**Response:** `"<filepaths>\n<paths>\n</filepaths>"` with 100 input / 10 output tokens.

---

## 6. Disable Safety Classifier Thinking (`safety_check_skip`)

**What it does:** Keeps the `@ant/security` safety check itself, but strips the `thinking` block so the gateway evaluates safety without high reasoning effort. This saves latency/tokens without disabling the safety feature (previous design that returned `{"safety":"safe"}` would have disabled it).

**Detection** — `core/src/optimization/detection.rs:is_safety_classifier_request`:

* `tools` absent, `system` exists.
* Lowercased `system` (supports String/Array via `extract_system_text`) contains `safety` **and** (`classifier` or `policy`), plus one of `safe` / `harmful` / `allowed`.

**Handling** — `core/src/optimization/mod.rs:maybe_strip_safety_thinking` + `proxy/src/server/handler.rs:handle_proxy`:

* When `enable_safety_check_skip == true`, the `thinking` (and legacy `budget_tokens`) field is removed from the JSON body before `anthropic_to_openai_request`, then the request is forwarded normally with reduced reasoning. Tracing logs `Safety classifier: thinking stripped`.

---

## 7. Web Tool Intercept (`web_tool_intercept` — `web_search` / `web_fetch`)

**What it does:** Executes `web_search` and `web_fetch` tool calls locally so Claude can browse without an external MCP server. Disabled by default.

**Trigger** — `core/src/optimization/web_tools.rs:extract_latest_web_tool_call`:

* Scans `messages` in reverse for the latest block with `type == "tool_use"` and `name` in `{web_fetch, web_search}` (case-insensitive). Extracts `id` and `input`.

**Policy** — `core/src/optimization/web_tools.rs:WebFetchEgressPolicy` / `core/src/core/config.rs`:

| Setting | Field | Default |
| --- | --- | --- |
| Allowed schemes | `web_fetch_allowed_schemes` | `"http,https"` |
| Allow private networks | `web_fetch_allow_private_networks` | `false` |
| Enabled | `enable_web_server_tools` | `false` |

* `validate_url` enforces scheme allowlist and, when `allow_private_networks == false`, blocks `localhost`, `*.localhost`, `*.local`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `127.0.0.0/8`, `::1`, `::ffff:127.0.0.1`, carrier-grade NAT (`100.64.0.0/10`), documentation / benchmark ranges (`192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `198.18.0.0/15`), multicast, etc. — via `is_private_address` + `is_public_ip`/`is_public_ipv4`.
* DNS resolution re-checks resolved IPs against the same policy (`resolve_target`).
* Normalizes comma-separated schemes to lowercase.

**Execution:**

* `web_search`: requires `query` (or `q`), encodes it, fetches `https://duckduckgo.com/html/?q=<encoded>` via the same fetch pipeline.
* `web_fetch`: requires `url`, validates against policy, then `fetch_url`.

**Fetch pipeline — `web_tools::fetch_url`:**

* Follows up to 5 redirects (`MAX_REDIRECTS`), resolving redirects relative to the current URL.
* Uses a pinned `reqwest` client (`resolve(host, addr)`, no automatic redirects, 60 s timeout = `crate::constants::HTTP_TIMEOUT_SECS`).
* Caps body at 2 MiB (`MAX_WEB_FETCH_BYTES`) via streaming — `collect_limited` returns an error if exceeded.
* Strips HTML tags naively (`<...>` removal), truncates to 20 000 chars (`truncate_chars`), then formats as:
  ```
  URL: <url>
  Status: <status>
  Content-Type: <type>

  <body>
  ```

**Response:** Fetched text (or an error string such as `"Private network access is not allowed: ..."`) with 100 input / 100 output tokens.

---

## Additional setting

**API Call Logging (`enable_api_call_logging`):** When enabled, appends JSON Lines to `{local_app_data}/FreeClaudeDesktop/logs/api-calls.log` via `proxy/src/server/api_log.rs` (10 MiB per file + 4 archives). Disabled by default.

---

## References

* Handler dispatch: `core/src/optimization/mod.rs`
* Detection: `core/src/optimization/detection.rs`
* Command parsing: `core/src/optimization/command_utils.rs`
* Web tools & egress policy: `core/src/optimization/web_tools.rs`
* Settings schema & defaults: `core/src/core/config.rs`
* Tests: `cargo test --workspace --locked` (covers all six handlers, private-network guards, and the 2 MiB guard)
