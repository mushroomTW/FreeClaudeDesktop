import { expect, test } from "bun:test";

// 這一組測試以最小可用的 DOM/Local Storage 替身，實際載入瀏覽器端的
// proxy/src/server/dashboard.js（而非切片段落），驗證認證後的載入流程、
// 各錯誤分支與互動行為。腳本會在被 import 時立即執行一次載入，因此
// 替身必須先安裝完成。

function createElement(tag = "div") {
  const listeners = new Map();
  const classes = new Set();
  return {
    tagName: tag.toUpperCase(),
    style: {},
    dataset: {},
    classList: {
      add: (...names) => names.forEach((name) => classes.add(name)),
      remove: (...names) => names.forEach((name) => classes.delete(name)),
      toggle: (name) => (classes.has(name) ? classes.delete(name) : classes.add(name)),
      contains: (name) => classes.has(name)
    },
    value: "",
    textContent: "",
    placeholder: "",
    checked: false,
    innerHTML: "",
    disabled: false,
    title: "",
    className: "",
    children: [],
    appendChild(child) {
      this.children.push(child);
      return child;
    },
    append(...nodes) {
      this.children.push(...nodes);
    },
    replaceChildren(...nodes) {
      this.children = nodes;
    },
    addEventListener(type, handler) {
      listeners.set(type, handler);
    },
    removeEventListener(type) {
      listeners.delete(type);
    },
    dispatch(type, event) {
      const handler = listeners.get(type);
      return handler ? handler(event) : undefined;
    },
    querySelectorAll: () => [],
    querySelector: () => null,
    click() {},
    remove() {},
    focus() {},
    select() {},
    scrollIntoView() {},
    setAttribute() {},
    getAttribute: () => null,
    hasAttribute: () => false,
    removeAttribute() {},
    contains: () => false,
    getBoundingClientRect: () => ({ width: 0, height: 0, top: 0, left: 0 })
  };
}

const elements = new Map();
const timers = [];
let fetchImpl = async () => jsonResponse({});

function elementById(id) {
  if (!elements.has(id)) elements.set(id, createElement());
  return elements.get(id);
}

function jsonResponse(payload, { ok = true, status = 200, statusText = "OK" } = {}) {
  return {
    ok,
    status,
    statusText,
    json: async () => payload,
    text: async () => JSON.stringify(payload)
  };
}

function runNextTimer() {
  const run = timers.shift();
  if (run) run();
}

function flushTimers() {
  while (timers.length > 0) runNextTimer();
}

// 模擬 HTML 中既有的可排序表頭，腳本載入時會綁定點擊事件。
const sortableHeader = createElement("th");
sortableHeader.dataset.sort = "name";

globalThis.document = {
  getElementById: elementById,
  createElement: (tag) => createElement(tag),
  querySelectorAll: (selector) => (selector === ".sortable-th" ? [sortableHeader] : []),
  querySelector: () => null,
  documentElement: createElement("html"),
  body: createElement("body"),
  title: "",
  addEventListener() {}
};
globalThis.window = {
  location: { hash: "", pathname: "/dashboard" },
  addEventListener() {},
  matchMedia: () => ({ matches: false, addEventListener() {} })
};
globalThis.history = { replaceState() {} };

const localStore = new Map();
const sessionStore = new Map();
function storage(map) {
  return {
    getItem: (key) => (map.has(key) ? map.get(key) : null),
    setItem: (key, value) => map.set(key, String(value)),
    removeItem: (key) => map.delete(key)
  };
}
globalThis.localStorage = storage(localStore);
globalThis.sessionStorage = storage(sessionStore);

globalThis.confirm = () => true;
globalThis.alert = () => {};
globalThis.setTimeout = (run) => timers.push(run);
globalThis.clearTimeout = () => {};
globalThis.fetch = (path, options) => fetchImpl(path, options);

const settings = {
  baseUrl: "https://gateway.example/v1",
  authScheme: "bearer",
  hasApiKey: true,
  customClaudePath: "C:/Claude/Claude.exe",
  realModelSonnet: "claude-sonnet-4-5",
  realModelOpus: "claude-opus-4-5",
  realModelHaiku: "claude-haiku-4-5",
  realModel: "claude-sonnet-4-5",
  discoveredModels: ["gpt-4o", "gpt-4o-mini"],
  transportType: "openai_chat",
  reasoningReplayMode: "inline",
  enableQuotaCheckMock: false,
  enablePrefixDetection: false,
  enableTitleGenerationSkip: false,
  enableSuggestionModeSkip: false,
  enableFilepathExtractionMock: false,
  enableSafetyCheckSkip: false,
  enableApiCallLogging: true,
  enableWebServerTools: true,
  webFetchAllowedSchemes: "http,https",
  webFetchAllowPrivateNetworks: true,
  themeMode: "dark",
  language: "en"
};

const healthyStatus = {
  proxy: { port: 4321 },
  mcp: { hasBundledSkills: true, serverCount: 2, serverNames: ["filesystem", "git"] }
};

let rpcPayload = { result: { path: "C:/Claude/Claude.exe" } };
let rpcError = null;

function defaultFetch(path) {
  if (path === "/settings") return jsonResponse(settings);
  if (path === "/status") return jsonResponse(healthyStatus);
  if (path === "/rpc") {
    if (rpcError) return Promise.reject(rpcError);
    return jsonResponse(rpcPayload);
  }
  return jsonResponse({});
}

fetchImpl = defaultFetch;

// 預先放入已儲存的主題，讓 applyTheme 走「非 system」分支（與伺服器設定一致）。
localStore.set("theme", "dark");

// 載入腳本；尾端會立即觸發一次 load()。
await import("../proxy/src/server/dashboard.js");
await Bun.sleep(20);

test("首次載入會套用設定、更新 MCP 狀態與偵測路徑", () => {
  expect(elementById("activePort").textContent).toBe("127.0.0.1 : 4321");
  expect(elementById("baseUrl").value).toBe("https://gateway.example/v1");
  expect(elementById("authScheme").value).toBe("bearer");
  expect(elementById("realModelSonnet").value).toBe("claude-sonnet-4-5");
  expect(elementById("transportType").value).toBe("openai_chat");
  expect(elementById("reasoningReplayMode").value).toBe("inline");
  expect(elementById("enableApiCallLogging").checked).toBe(true);
  expect(elementById("enableQuotaCheckMock").checked).toBe(false);
  expect(elementById("enableWebServerTools").checked).toBe(true);
  expect(elementById("webToolsSettings").classList.contains("hidden")).toBe(false);
  expect(elementById("webFetchAllowedSchemes").value).toBe("http,https");
  expect(elementById("webFetchAllowPrivateNetworks").checked).toBe(true);
  expect(elementById("language").value).toBe("en");
  expect(document.documentElement.dataset.theme).toBe("dark");

  // 已提供設定的模型清單會逐一建立 <option>。
  expect(elementById("modelSuggestions").children).toHaveLength(2);

  // MCP：已偵測到 bundled skills → 綠點，並列出兩個伺服器。
  expect(elementById("mcpBundledDot").className).toBe("status-dot online");
  expect(elementById("mcpServerCount").textContent).toContain("2");
  expect(elementById("mcpServerList").children).toHaveLength(2);

  // 偵測到的 Claude 路徑（online 分支）。
  expect(elementById("detectedClaudePath").textContent).toBe("C:/Claude/Claude.exe");
  expect(elementById("detectedClaudeDot").className).toBe("status-dot online");

  // 認證通過後顯示主要版面，並提示載入成功。
  expect(elementById("authWrapper").classList.contains("hidden")).toBe(true);
  expect(elementById("mainLayout").classList.contains("hidden")).toBe(false);
  const toast = elementById("toastContainer").children.at(-1);
  expect(toast.className).toBe("toast success");
  expect(toast.children).toHaveLength(2);

  // showToast 以 setTimeout 切換顯示狀態。
  runNextTimer();
  expect(toast.classList.contains("show")).toBe(true);
  flushTimers();
  expect(toast.classList.contains("show")).toBe(false);
});

test("點擊 MCP 摘要可展開與收合伺服器清單", () => {
  const count = elementById("mcpServerCount");
  const list = elementById("mcpServerList");
  list.classList.add("hidden");

  count.dispatch("click");
  expect(list.classList.contains("hidden")).toBe(false);
  expect(count.textContent).toContain("▾");

  count.dispatch("click");
  expect(list.classList.contains("hidden")).toBe(true);
  expect(count.textContent).toContain("▸");
});

test("DetectClaude 未回傳路徑時顯示離線狀態", async () => {
  rpcPayload = {};
  await elementById("loadBtn").onclick();
  expect(elementById("detectedClaudeDot").className).toBe("status-dot offline");
  expect(elementById("detectedClaudeStatus").dataset.i18n).toBe("detected_offline_title");
  expect(elementById("detectedClaudePath").dataset.i18n).toBe("detected_offline");
  expect(elementById("detectedClaudePath").textContent.length).toBeGreaterThan(0);
});

test("DetectClaude 失敗時顯示失敗狀態", async () => {
  rpcError = new Error("rpc unavailable");
  await elementById("loadBtn").onclick();
  expect(elementById("detectedClaudeDot").className).toBe("status-dot failed");
  expect(elementById("detectedClaudeStatus").dataset.i18n).toBe("detected_failed_title");
});

test("沒有可用 MCP 伺服器時顯示離線點與空清單提示", async () => {
  rpcError = null;
  rpcPayload = { result: { path: "C:/Claude/Claude.exe" } };
  const bareStatus = { proxy: { port: 4321 }, mcp: { hasBundledSkills: false, serverCount: 0 } };
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) => (path === "/status" ? jsonResponse(bareStatus) : baseFetch(path, options));
  await elementById("loadBtn").onclick();
  expect(elementById("mcpBundledDot").className).toBe("status-dot offline");
  expect(elementById("mcpServerCount").textContent).toContain("0");
  expect(elementById("mcpServerList").children).toHaveLength(1);
  fetchImpl = baseFetch;
});

test("未帶 MCP 資訊的狀態視為中性（有伺服器但未偵測）", async () => {
  const neutralStatus = { proxy: { port: 9999 }, mcp: { hasBundledSkills: false, serverCount: 3 } };
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) => (path === "/status" ? jsonResponse(neutralStatus) : baseFetch(path, options));
  await elementById("loadBtn").onclick();
  expect(elementById("mcpBundledDot").className).toBe("status-dot neutral");
  fetchImpl = baseFetch;
});

test("設定讀取失敗時顯示錯誤提示並維持主要版面", async () => {
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) => {
    if (path === "/settings") {
      return jsonResponse({ error: "gateway unreachable", code: "gateway_url_errors" }, { ok: false, status: 502, statusText: "Bad Gateway" });
    }
    return baseFetch(path, options);
  };
  await elementById("loadBtn").onclick();
  expect(elementById("mainLayout").classList.contains("hidden")).toBe(false);
  const toast = elementById("toastContainer").children.at(-1);
  expect(toast.className).toBe("toast error");
  // 伺服器回應帶 code 時，tError 會優先採用語系表中的對照字串。
  expect(toast.children.at(-1).textContent.length).toBeGreaterThan(0);
  fetchImpl = baseFetch;
});

test("401 會以認證提示取代原始回應內容", async () => {
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) =>
    path === "/settings" ? jsonResponse({}, { ok: false, status: 401, statusText: "Unauthorized" }) : baseFetch(path, options);
  await elementById("loadBtn").onclick();
  const toast = elementById("toastContainer").children.at(-1);
  expect(toast.className).toBe("toast error");
  expect(toast.children.at(-1).textContent.length).toBeGreaterThan(0);
  fetchImpl = baseFetch;
});

test("切換 Web 工具攔截會顯示或隱藏工具設定", () => {
  const toggle = elementById("enableWebServerTools");
  const panel = elementById("webToolsSettings");

  toggle.dispatch("change", { target: { checked: false } });
  expect(panel.classList.contains("hidden")).toBe(true);

  toggle.dispatch("change", { target: { checked: true } });
  expect(panel.classList.contains("hidden")).toBe(false);
});

test("語言切換會重新套用語系並重繪模型表", () => {
  const select = elementById("language");
  select.value = "zh-tw";
  select.onchange();
  expect(document.documentElement.lang).toBe("zh-TW");
  expect(document.title).toContain("Dashboard");
});

test("未啟用 Web 工具時會隱藏工具設定區塊", async () => {
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) =>
    path === "/settings"
      ? jsonResponse({ ...settings, enableWebServerTools: false })
      : baseFetch(path, options);

  await elementById("loadBtn").onclick();

  expect(elementById("enableWebServerTools").checked).toBe(false);
  expect(elementById("webToolsSettings").classList.contains("hidden")).toBe(true);
  fetchImpl = baseFetch;
});

test("沒有可用模型時會顯示提示列", async () => {
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) =>
    path === "/settings"
      ? jsonResponse({ ...settings, discoveredModels: [] })
      : baseFetch(path, options);

  await elementById("loadBtn").onclick();

  const body = elementById("modelsTableBody");
  expect(body.children).toHaveLength(1);
  expect(body.children[0].children[0].textContent.length).toBeGreaterThan(0);
  fetchImpl = baseFetch;
});

test("點擊可排序表頭會依鍵值重新排序模型", async () => {
  const baseFetch = fetchImpl;
  fetchImpl = async (path, options) =>
    path === "/settings"
      ? jsonResponse({
          ...settings,
          discoveredModels: ["gpt-4o", "gpt-4o-mini", "claude-x"]
        })
      : baseFetch(path, options);

  await elementById("loadBtn").onclick();
  const rowsBefore = elementById("modelsTableBody").children.length;

  sortableHeader.onclick();
  expect(elementById("modelsTableBody").children.length).toBe(rowsBefore);

  // 第二次點擊會切換升降冪方向，走過另一半的比較分支。
  sortableHeader.onclick();
  expect(elementById("modelsTableBody").children.length).toBe(rowsBefore);
  fetchImpl = baseFetch;
});
