import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const source = readFileSync(new URL("../proxy/src/server/dashboard.js", import.meta.url), "utf8");
// 使用實際頁面的認證初始化與 request 呼叫邊界，重現同分頁的首次載入與重新整理。
const bootstrap = source.slice(0, source.indexOf("    // i18n"));
const request = source.slice(source.indexOf("    async function request("), source.indexOf("    // Toggle Web Fetch"));
function page(hash, stored, response = { ok: true, status: 200, json: async () => ({}) }) {
  const calls = [];
  const location = { hash, pathname: "/dashboard" };
  const context = vm.createContext({
    window: { location },
    history: { replaceState: () => { location.hash = ""; } },
    document: { title: "Dashboard", getElementById: () => ({}) },
    sessionStorage: { getItem: (key) => stored.get(key) ?? null, setItem: (key, value) => stored.set(key, value) },
    fetch: async (path, options) => { calls.push({ path, options }); return response; },
    t: (key) => key
  });
  vm.runInContext(bootstrap + request, context);
  return { calls, location, request: () => vm.runInContext("request('/settings')", context) };
}

test("Dashboard 同分頁重新整理保留驗證 token，網址仍移除 token", async () => {
  const stored = new Map();
  const initial = page("#test-token", stored);
  await initial.request();
  expect(initial.calls[0].options.headers.Authorization).toBe("Bearer test-token");
  expect(initial.location.hash).toBe("");
  const reloaded = page("", stored);
  await reloaded.request();
  expect(reloaded.calls[0].options.headers.Authorization).toBe("Bearer test-token");
});

test("Dashboard 導覽 fragment 不會覆寫已儲存的驗證 token", async () => {
  const stored = new Map();
  await page("#test-token", stored).request();
  const navigated = page("#models", stored);
  await navigated.request();
  expect(navigated.calls[0].options.headers.Authorization).toBe("Bearer test-token");
});

test("空白的 401 回應應顯示重新開啟控制台提示", async () => {
  const denied = page("", new Map(), { ok: false, status: 401, json: async () => { throw new SyntaxError("Unexpected end of JSON input"); } });
  await expect(denied.request()).rejects.toThrow("toast_auth_required");
});
