#!/usr/bin/env bun

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const { globalInstallation, cleanupGlobalAliases } = require("./uninstall.cjs");

const PLATFORM_PACKAGES = {
  "darwin-arm64": "@mushroomtw/freeclaudedesktop-darwin-arm64",
  "darwin-x64": "@mushroomtw/freeclaudedesktop-darwin-x64",
  "linux-arm64": "@mushroomtw/freeclaudedesktop-linux-arm64",
  "linux-x64": "@mushroomtw/freeclaudedesktop-linux-x64",
  "win32-arm64": "@mushroomtw/freeclaudedesktop-win32-arm64",
  "win32-x64": "@mushroomtw/freeclaudedesktop-win32-x64"
};

function platformPackageDirectory() {
  const key = `${process.platform}-${process.arch}`;
  const packageName = PLATFORM_PACKAGES[key];
  if (!packageName) {
    throw new Error(`尚未提供 ${key} 平台的 FreeClaudeDesktop binary。`);
  }
  try {
    return path.dirname(require.resolve(`${packageName}/package.json`));
  } catch {
    throw new Error(`找不到 ${packageName}。請重新執行 bun add -g @mushroomtw/freeclaudedesktop。`);
  }
}

function nativeCliPath() {
  const filename = process.platform === "win32" ? "freeclaude.exe" : "freeclaude";
  const binary = path.join(platformPackageDirectory(), "bin", filename);
  if (!fs.existsSync(binary)) {
    throw new Error(`找不到管理程式：${binary}`);
  }
  return binary;
}

function runNative(args) {
  const result = spawnSync(nativeCliPath(), args, {
    env: process.env,
    stdio: "inherit"
  });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
}

function printHelp() {
  console.log(`FreeClaudeDesktop Bun 管理工具

用法：freeclaude <命令>

命令：
  start      啟動本機 Proxy 並等待健康檢查
  stop       停止由 FreeClaudeDesktop 管理的 Proxy
  restart    重新啟動本機 Proxy
  status     顯示 Proxy 與自動啟動狀態
  dashboard  開啟 Web 控制台
  path       顯示目前平台 binary 所在資料夾
  purge      停止服務、還原 Claude 設定並完整清除本程式資料
  uninstall  清理本機狀態，並移除目前的 Bun 全域套件
  <其他>     直接傳遞給原生 freeclaude CLI
`);
}

function uninstall(args) {
  // --help 等參數仍交由原生 CLI 處理，但不得因此移除套件。
  const helpOnly = args.some((arg) => arg === "--help" || arg === "-h");
  const installation = helpOnly ? null : globalInstallation(__dirname);
  runNative(["uninstall", ...args]);
  if (process.exitCode !== 0 || helpOnly) return;
  const retry = "bun remove -g @mushroomtw/freeclaudedesktop --ignore-scripts";
  if (installation.kind === "local") return;
  if (installation.kind === "unknown") {
    console.error(`本機狀態已清理，但無法確認 Bun 全域安裝位置：${installation.reason}。如需移除全域套件，請執行 ${retry}`);
    process.exitCode = 1;
    return;
  }
  // 原生清理已退出，Windows 的 Rust binary 鎖定也已釋放。
  // 固定已確認的安裝根目錄，避免呼叫端的 bunfig.toml 影響移除對象。
  const result = spawnSync(process.execPath, ["remove", "-g", "@mushroomtw/freeclaudedesktop", "--ignore-scripts"], {
    cwd: installation.directory,
    env: {
      ...process.env,
      BUN_INSTALL_GLOBAL_DIR: installation.directory,
      BUN_INSTALL_BIN: installation.binDirectory
    },
    stdio: "inherit"
  });
  process.exitCode = result.status ?? 1;
  if (result.error || process.exitCode !== 0) {
    console.error(`本機狀態已清理，但套件移除失敗${result.error ? `：${result.error.message}` : ""}。請重試 ${retry}`);
  } else {
    // Bun 在部分平台會留下全域命令入口，只清理由本次安裝持有的別名。
    try {
      cleanupGlobalAliases(installation);
    } catch (error) {
      console.error(`套件已移除，但命令入口清理失敗：${error.message}。請檢查 ${installation.binDirectory} 內殘留的 freeclaude / freecd 別名。`);
      process.exitCode = 1;
    }
  }
}

const [command, ...rest] = process.argv.slice(2);
try {
  switch (command) {
    case undefined:
    case "help":
    case "--help":
    case "-h":
      printHelp();
      break;
    case "restart":
      runNative(["stop"]);
      if (process.exitCode === 0) runNative(["start"]);
      break;
    case "dashboard":
      runNative(["configure", ...rest]);
      break;
    case "path":
      console.log(platformPackageDirectory());
      break;
    case "purge":
      runNative(["purge", "--yes", ...rest]);
      break;
    case "uninstall":
      uninstall(rest);
      break;
    default:
      runNative([command, ...rest]);
      break;
  }
} catch (error) {
  console.error(`freeclaude：${error.message}`);
  process.exitCode = 1;
}
