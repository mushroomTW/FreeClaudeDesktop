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
    throw new Error(`No FreeClaudeDesktop binary is available for the ${key} platform.`);
  }
  try {
    return path.dirname(require.resolve(`${packageName}/package.json`));
  } catch {
    throw new Error(`Cannot find ${packageName}. Run bun add -g @mushroomtw/freeclaudedesktop again.`);
  }
}

function nativeCliPath() {
  const filename = process.platform === "win32" ? "freeclaude.exe" : "freeclaude";
  const binary = path.join(platformPackageDirectory(), "bin", filename);
  if (!fs.existsSync(binary)) {
    throw new Error(`Management program not found: ${binary}`);
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
  console.log(`FreeClaudeDesktop Bun management tool

Usage: freeclaude <command>

Commands:
  start      Start the local Proxy and wait for the health check
  stop       Stop the Proxy managed by FreeClaudeDesktop
  restart    Restart the local Proxy
  status     Show the Proxy and autostart status
  dashboard  Open the web dashboard
  path       Show the folder containing the platform binary
  purge      Stop services, restore Claude settings, and erase all program data
  uninstall  Clean up local state and remove the current Bun global package
  <other>    Pass the arguments through to the native freeclaude CLI
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
    console.error(`Local state has been cleaned up, but the Bun global installation location could not be determined: ${installation.reason}. To remove the global package, run ${retry}`);
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
    const reason = result.error ? `: ${result.error.message}` : "";
    console.error(`Local state has been cleaned up, but removing the package failed${reason}. Retry with ${retry}`);
  } else {
    // Bun 在部分平台會留下全域命令入口，只清理由本次安裝持有的別名。
    try {
      cleanupGlobalAliases(installation);
    } catch (error) {
      console.error(`The package was removed, but cleaning up command entries failed: ${error.message}. Check ${installation.binDirectory} for leftover freeclaude / freecd aliases.`);
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
  console.error(`freeclaude: ${error.message}`);
  process.exitCode = 1;
}
