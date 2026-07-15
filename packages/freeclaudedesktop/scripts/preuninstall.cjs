const path = require("node:path");
const { spawnSync } = require("node:child_process");

const wrapper = path.join(__dirname, "..", "bin", "freeclaude-proxy.cjs");
const result = spawnSync(process.execPath, [wrapper, "purge"], { stdio: "inherit" });

if (result.error) throw result.error;
if (result.status !== 0) {
  throw new Error("完整清除失敗，已中止解除安裝以保留可修復的 FreeClaudeDesktop。");
}
