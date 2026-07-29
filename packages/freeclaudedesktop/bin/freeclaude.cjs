#!/usr/bin/env node

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

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
    throw new Error(`找不到 ${packageName}。請重新執行 npm install -g @mushroomtw/freeclaudedesktop。`);
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
  console.log(`FreeClaudeDesktop npm 管理工具

用法：freeclaude <命令>

命令：
  start      啟動本機 Proxy 並等待健康檢查
  stop       停止由 FreeClaudeDesktop 管理的 Proxy
  restart    重新啟動本機 Proxy
  status     顯示 Proxy 與自動啟動狀態
  dashboard  開啟 Web 控制台
  path       顯示目前平台 binary 所在資料夾
  purge      停止服務、還原 Claude 設定並完整清除本程式資料
  <其他>     直接傳遞給原生 freeclaude CLI
`);
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
    default:
      runNative([command, ...rest]);
      break;
  }
} catch (error) {
  console.error(`freeclaude：${error.message}`);
  process.exitCode = 1;
}
