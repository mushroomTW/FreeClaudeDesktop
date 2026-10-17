import { expect, test } from "bun:test";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

// freeclaude.cjs 是 Bun 包裝腳本，載入時即依 process.argv 執行。
// 這裡只驅動不會呼叫原生執行檔的分支（說明與 path）；其餘命令會實際
// 啟動已安裝的 freeclaude.exe，因此刻意不在測試中觸發。
const require = createRequire(import.meta.url);
const wrapperPath = require.resolve("../packages/freeclaudedesktop/bin/freeclaude.cjs");

function runWrapper(args) {
  const lines = [];
  const originalLog = console.log;
  const originalArgv = process.argv;
  console.log = (...parts) => lines.push(parts.map(String).join(" "));
  process.argv = [process.execPath, wrapperPath, ...args];
  try {
    require(wrapperPath);
  } finally {
    console.log = originalLog;
    process.argv = originalArgv;
    delete require.cache[wrapperPath];
  }
  return lines;
}

test("未指定命令時印出使用說明", () => {
  const output = runWrapper([]).join("\n");
  expect(output).toContain("Usage: freeclaude <command>");
  expect(output).toContain("uninstall  Clean up local state");
});

test("--help 也會印出使用說明", () => {
  expect(runWrapper(["--help"]).join("\n")).toContain("Usage: freeclaude <command>");
});

test("path 會印出平台套件目錄", () => {
  const printed = runWrapper(["path"]).at(-1);
  expect(path.isAbsolute(printed)).toBe(true);
  expect(fs.existsSync(path.join(printed, "package.json"))).toBe(true);
  expect(fs.existsSync(path.join(printed, "bin"))).toBe(true);
});
