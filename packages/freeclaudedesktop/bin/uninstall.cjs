const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { spawn, spawnSync } = require("node:child_process");

function absoluteDirectory(value, home) {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("Global directory setting is not a valid path");
  }
  if (value === "~") return home;
  if (value.startsWith("~/") || value.startsWith("~\\")) return path.resolve(home, value.slice(2));
  if (!path.isAbsolute(value)) throw new Error("Global directory uses a relative path");
  return path.normalize(value);
}

function configuredGlobalDirectory() {
  const home = os.homedir();
  if (process.env.BUN_INSTALL_GLOBAL_DIR) {
    return absoluteDirectory(process.env.BUN_INSTALL_GLOBAL_DIR, home);
  }
  const files = new Set([path.join(home, ".bunfig.toml")]);
  if (process.env.XDG_CONFIG_HOME) files.add(path.join(process.env.XDG_CONFIG_HOME, ".bunfig.toml"));
  const directories = new Set();
  for (const file of files) {
    if (!fs.existsSync(file)) continue;
    const value = Bun.TOML.parse(fs.readFileSync(file, "utf8")).install?.globalDir;
    if (value !== undefined) directories.add(absoluteDirectory(value, home));
  }
  // 有衝突的全域設定時不推測優先順序，保留套件供使用者手動移除。
  if (directories.size > 1) throw new Error("globalDir settings in multiple global bunfig.toml files disagree");
  if (directories.size === 1) return [...directories][0];
  const install = process.env.BUN_INSTALL ? absoluteDirectory(process.env.BUN_INSTALL, home) : path.join(home, ".bun");
  return path.join(install, "install", "global");
}

function sameDirectory(left, right) {
  if (fs.realpathSync(left) === fs.realpathSync(right)) return true;
  // Windows 的長路徑、8.3 短路徑及大小寫可能指向同一資料夾。
  const a = fs.statSync(left, { bigint: true });
  const b = fs.statSync(right, { bigint: true });
  return a.ino !== 0n && a.dev === b.dev && a.ino === b.ino;
}

// Windows 的 Bun shim 以 <entry>.bunx 記錄原始入口，據此判斷別名是否由本套件擁有。
function windowsOwnedAliases(binDirectory, entry, current) {
  const metadata = `${entry}.bunx`;
  if (!fs.existsSync(metadata)) return [];
  const target = fs.readFileSync(metadata).toString("utf16le").split('"\0')[0];
  const resolved = path.resolve(binDirectory, "..", target);
  if (!fs.existsSync(resolved) || !sameDirectory(resolved, path.join(current, "bin/freeclaude.cjs"))) return [];
  const owned = [];
  for (const filename of [metadata, `${entry}.exe`]) {
    if (fs.existsSync(filename)) owned.push({ filename, identity: fs.lstatSync(filename, { bigint: true }) });
  }
  return owned;
}

function posixOwnedAliases(entry, current) {
  if (!fs.existsSync(entry) || !sameDirectory(entry, path.join(current, "bin/freeclaude.cjs"))) return [];
  return [{ filename: entry, identity: fs.lstatSync(entry, { bigint: true }) }];
}

function ownedAliases(binDirectory, current) {
  const aliases = [];
  for (const name of ["freeclaude", "freecd"]) {
    const entry = path.join(binDirectory, name);
    aliases.push(...(process.platform === "win32"
      ? windowsOwnedAliases(binDirectory, entry, current)
      : posixOwnedAliases(entry, current)));
  }
  return aliases;
}

function retireWindowsAlias(filename) {
  // 正在執行的 Windows shim 無法 unlink，但可以在同一磁碟重新命名。
  const directory = fs.mkdtempSync(path.join(path.dirname(filename), ".freecd-uninstall-"));
  const retired = path.join(directory, path.basename(filename));
  try {
    fs.renameSync(filename, retired);
  } catch (error) {
    fs.rmdirSync(directory);
    throw error;
  }
  const identity = fs.statSync(retired, { bigint: true });
  // 子程序僅刪除已驗證的暫存檔與空目錄，並等待父層 shim 釋放鎖定。
  const cleanup = `
    const fs = require("node:fs");
    const filename = ${JSON.stringify(retired)};
    const directory = ${JSON.stringify(directory)};
    for (let attempt = 0; attempt < 100; attempt++) {
      await Bun.sleep(50);
      try {
        const stat = fs.statSync(filename, { bigint: true });
        if (stat.dev !== BigInt(${JSON.stringify(String(identity.dev))}) || stat.ino !== BigInt(${JSON.stringify(String(identity.ino))})) process.exit(1);
        fs.unlinkSync(filename);
        fs.rmdirSync(directory);
        process.exit(0);
      } catch (error) {
        if (error.code === "ENOENT") {
          try { fs.rmdirSync(directory); } catch (cleanupError) { if (cleanupError.code !== "ENOENT") break; }
          process.exit(0);
        }
        if (error.code !== "EPERM" && error.code !== "EBUSY" && error.code !== "EACCES") break;
      }
    }
    console.error("The package and command entry were removed, but cleaning up the temporary file failed. Delete it after the command exits: " + filename);
    process.exit(1);
  `;
  const child = spawn(process.execPath, ["--eval", cleanup], {
    cwd: path.dirname(directory),
    env: process.env,
    detached: true,
    windowsHide: true,
    stdio: "inherit"
  });
  child.on("error", (error) => {
    console.error(`The package and command entry were removed, but the temporary file cleanup could not be started: ${error.message}. Delete ${retired} after the command exits`);
    process.exitCode = 1;
  });
  child.unref();
}

// 讀取別名目前的身分；已消失（ENOENT）時回傳 null 交由呼叫端略過。
function currentAliasIdentity(filename) {
  try {
    return fs.lstatSync(filename, { bigint: true });
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

// 只有 Windows 上被鎖定的 .exe 才需要延後由子程序清理。
function isRetirableWindowsAlias(filename, error) {
  if (process.platform !== "win32") return false;
  if (!filename.endsWith(".exe")) return false;
  return ["EPERM", "EBUSY", "EACCES"].includes(error.code);
}

function removeAliasFile(filename) {
  try {
    fs.unlinkSync(filename);
  } catch (error) {
    if (error.code === "ENOENT") return;
    if (!isRetirableWindowsAlias(filename, error)) throw error;
    retireWindowsAlias(filename);
  }
}

function cleanupGlobalAliases(installation) {
  for (const { filename, identity } of installation.aliases) {
    // lstat 也能檢查套件移除後已失效的 POSIX symlink。
    const current = currentAliasIdentity(filename);
    if (!current) continue;
    if (identity.dev !== current.dev || identity.ino !== current.ino) continue;
    removeAliasFile(filename);
  }
}

function globalInstallation(binDirectory) {
  try {
    const directory = configuredGlobalDirectory();
    const current = fs.realpathSync(path.join(binDirectory, ".."));
    const expected = path.join(directory, "node_modules", "@mushroomtw", "freeclaudedesktop");
    if (!fs.existsSync(expected)) return { kind: "local" };
    if (!sameDirectory(expected, current)) return { kind: "local" };
    // 全域連結到來源目錄時，不得從來源目錄啟動自我移除。
    for (let ancestor = current; !sameDirectory(ancestor, directory);) {
      const parent = path.dirname(ancestor);
      if (parent === ancestor) return { kind: "local" };
      ancestor = parent;
    }
    const manifest = JSON.parse(fs.readFileSync(path.join(directory, "package.json"), "utf8"));
    if (!manifest.dependencies?.["@mushroomtw/freeclaudedesktop"]) {
      throw new Error("The global package manifest does not include the current package");
    }
    const result = spawnSync(process.execPath, ["pm", "bin", "-g"], {
      cwd: directory,
      env: process.env,
      encoding: "utf8"
    });
    if (result.error || result.status !== 0) throw new Error("Could not query the Bun global command directory");
    const globalBin = absoluteDirectory(result.stdout.trim(), os.homedir());
    return { kind: "global", directory: fs.realpathSync(directory), binDirectory: globalBin, aliases: ownedAliases(globalBin, current) };
  } catch (error) {
    return { kind: "unknown", reason: error.message };
  }
}

module.exports = { globalInstallation, cleanupGlobalAliases };
