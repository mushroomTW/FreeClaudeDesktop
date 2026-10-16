import { afterEach, expect, test } from "bun:test";
import * as fs from "node:fs";
import path from "node:path";
import os from "node:os";
import vm from "node:vm";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { platforms } from "../scripts/platforms.mjs";
import { readArchive, verifyPackage } from "../scripts/verify-package.mjs";

const packageName = "@mushroomtw/freeclaudedesktop";
const packageDirectory = new URL("../packages/freeclaudedesktop/", import.meta.url);
const wrapperSource = fs.readFileSync(new URL("bin/freeclaude.cjs", packageDirectory), "utf8");
const uninstallSource = fs.readFileSync(new URL("bin/uninstall.cjs", packageDirectory), "utf8");
const manifest = JSON.parse(fs.readFileSync(new URL("package.json", packageDirectory), "utf8"));
const roots = [];
const realRequire = createRequire(import.meta.url);

function temporaryDirectory() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "freecd-bun-test-"));
  roots.push(root);
  return root;
}

afterEach(() => {
  for (const root of roots.splice(0)) {
    // 僅刪除本測試建立、且仍位於暫存目錄直屬位置的資料夾。
    if (path.dirname(root) !== path.resolve(os.tmpdir()) || !path.basename(root).startsWith("freecd-bun-test-")) throw new Error("測試清理路徑超出暫存範圍");
    fs.rmSync(root, { recursive: true, force: true });
  }
});

function write(filename, content) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, content);
}

function fixture({ local = false, platform = process.platform, arch = process.arch, config = "env" } = {}) {
  const root = temporaryDirectory();
  const home = path.join(root, "home");
  const globalDirectory = config === "default" ? path.join(home, ".bun/install/global") : path.join(root, "custom global");
  const binDirectory = path.join(root, "global bin");
  const current = local ? path.join(root, "source/packages/freeclaudedesktop") : path.join(globalDirectory, "node_modules/@mushroomtw/freeclaudedesktop");
  const nativeDirectory = path.join(root, "native");
  write(path.join(current, "package.json"), JSON.stringify(manifest));
  write(path.join(current, "bin/freeclaude.cjs"), wrapperSource);
  write(path.join(current, "bin/uninstall.cjs"), uninstallSource);
  write(path.join(nativeDirectory, "package.json"), JSON.stringify({ name: `${packageName}-${platform}-${arch}` }));
  write(path.join(nativeDirectory, "bin", platform === "win32" ? "freeclaude.exe" : "freeclaude"), "測試替身，不執行真實清理");
  write(path.join(globalDirectory, "package.json"), JSON.stringify({ dependencies: { [packageName]: manifest.version } }));
  const env = { BUN_INSTALL_BIN: binDirectory };
  if (config === "env") env.BUN_INSTALL_GLOBAL_DIR = globalDirectory;
  if (config === "toml") write(path.join(home, ".bunfig.toml"), `[install]\nglobalDir = ${JSON.stringify(globalDirectory)}\n`);
  if (config === "xdg") {
    env.XDG_CONFIG_HOME = path.join(root, "xdg");
    write(path.join(env.XDG_CONFIG_HOME, ".bunfig.toml"), `[install]\nglobalDir = ${JSON.stringify(globalDirectory)}\n`);
  }
  return { root, home, globalDirectory, binDirectory, current, nativeDirectory, env, platform, arch };
}

function runWrapper(f, args, options = {}) {
  const calls = [];
  const errors = [];
  const output = [];
  const fakeProcess = { platform: f.platform, arch: f.arch, execPath: process.execPath, argv: [process.execPath, path.join(f.current, "bin/freeclaude.cjs"), ...args], env: f.env };
  const execute = (command, argv, settings) => {
    calls.push({ command, args: argv, settings });
    if (command !== process.execPath) {
      return options.nativeError ? { error: new Error("原生啟動失敗"), status: null } : { status: options.nativeStatus ?? 0 };
    }
    if (argv[0] === "pm") return { status: options.queryStatus ?? 0, stdout: `${f.binDirectory}\n` };
    options.onRemove?.();
    return options.removeError ? { error: new Error("套件鎖定"), status: null } : { status: options.removeStatus ?? 0 };
  };
  const helper = { exports: {} };
  const require = (name) => {
    if (name === "node:child_process") return { spawnSync: execute };
    if (name === "node:os") return { homedir: () => f.home };
    if (name === "./uninstall.cjs") return helper.exports;
    return realRequire(name);
  };
  require.resolve = (name) => {
    if (options.missingPackage || name !== `${packageName}-${f.platform}-${f.arch}/package.json`) throw new Error("找不到平台套件");
    return path.join(f.nativeDirectory, "package.json");
  };
  const context = { require, module: helper, __dirname: path.join(f.current, "bin"), process: fakeProcess, console: { log: (value) => output.push(value), error: (value) => errors.push(value) }, Bun };
  vm.runInNewContext(uninstallSource, context, { filename: "uninstall.cjs" });
  vm.runInNewContext(wrapperSource.replace(/^#!.*\n/, ""), { ...context, module: { exports: {} } }, { filename: "freeclaude.cjs" });
  return { calls, errors, output, status: fakeProcess.exitCode ?? 0 };
}

test.each(Object.values(platforms))("平台解析與參數、退出碼傳遞：$suffix", (platform) => {
  const f = fixture({ platform: platform.os[0], arch: platform.cpu[0] });
  const result = runWrapper(f, ["status", "含 空白參數"], { nativeStatus: 17 });
  expect(result.status).toBe(17);
  expect(result.calls[0].args).toEqual(["status", "含 空白參數"]);
  expect(path.basename(result.calls[0].command)).toBe(platform.os[0] === "win32" ? "freeclaude.exe" : "freeclaude");
});

test.each(["env", "toml", "xdg", "default"])("全域解除安裝先清理再移除，目錄來源：%s", (config) => {
  const f = fixture({ config });
  const result = runWrapper(f, ["uninstall", "--yes"]);
  expect(result.status).toBe(0);
  expect(result.calls.map((call) => call.args[0])).toEqual(["pm", "uninstall", "remove"]);
  expect(result.calls[1].args).toEqual(["uninstall", "--yes"]);
  expect(result.calls[2].args).toEqual(["remove", "-g", packageName, "--ignore-scripts"]);
  expect(fs.realpathSync(result.calls[2].settings.cwd)).toBe(fs.realpathSync(f.globalDirectory));
  expect(result.calls[2].settings.env.BUN_INSTALL_BIN).toBe(f.binDirectory);
});

test("原生清理失敗不移除套件，並保留退出碼", () => {
  const result = runWrapper(fixture(), ["uninstall"], { nativeStatus: 9 });
  expect(result.status).toBe(9);
  expect(result.calls.some((call) => call.args[0] === "remove")).toBe(false);
});

test("原生程序無法啟動不移除套件", () => {
  const result = runWrapper(fixture(), ["uninstall"], { nativeError: true });
  expect(result.status).toBe(1);
  expect(result.calls.some((call) => call.args[0] === "remove")).toBe(false);
});

test.each([{ removeStatus: 12 }, { removeError: true }])("移除失敗提供可重試命令：%j", (options) => {
  const result = runWrapper(fixture(), ["uninstall"], options);
  expect(result.status).not.toBe(0);
  expect(result.errors.join("\n")).toContain("本機狀態已清理，但套件移除失敗");
  expect(result.errors.join("\n")).toContain(`bun remove -g ${packageName} --ignore-scripts`);
});

test("來源與區域安裝不移除其他全域套件", () => {
  const f = fixture({ local: true });
  write(path.join(f.globalDirectory, "node_modules/@mushroomtw/freeclaudedesktop/package.json"), JSON.stringify(manifest));
  const result = runWrapper(f, ["uninstall"]);
  expect(result.status).toBe(0);
  expect(result.calls.map((call) => call.args[0])).toEqual(["uninstall"]);
});

test("沒有全域安裝時，來源解除安裝正常完成", () => {
  const f = fixture({ local: true });
  f.env.BUN_INSTALL_GLOBAL_DIR = path.join(f.root, "不存在的全域目錄");
  expect(runWrapper(f, ["uninstall"]).status).toBe(0);
});

function aliasFiles(f, name, owner = f.current) {
  const entry = path.join(f.binDirectory, name);
  const target = path.join(owner, "bin/freeclaude.cjs");
  if (process.platform === "win32") {
    write(`${entry}.bunx`, Buffer.from(`${path.relative(path.dirname(f.binDirectory), target)}"\0bun `, "utf16le"));
    write(`${entry}.exe`, "測試 shim");
    return [`${entry}.bunx`, `${entry}.exe`];
  }
  fs.mkdirSync(f.binDirectory, { recursive: true });
  fs.symlinkSync(target, entry);
  return [entry];
}

test("移除自己持有的別名，保留指向其他安裝的同名命令", () => {
  const f = fixture();
  const foreign = path.join(f.root, "other-package");
  write(path.join(foreign, "bin/freeclaude.cjs"), "其他套件");
  const own = aliasFiles(f, "freecd");
  const other = aliasFiles(f, "freeclaude", foreign);
  expect(runWrapper(f, ["uninstall"]).status).toBe(0);
  for (const filename of own) expect(fs.existsSync(filename)).toBe(false);
  for (const filename of other) expect(fs.existsSync(filename)).toBe(true);
});

test("辨識後被其他程序替換的別名不得刪除", () => {
  const f = fixture();
  const aliases = aliasFiles(f, "freecd");
  const result = runWrapper(f, ["uninstall"], { onRemove: () => {
    for (const filename of aliases) {
      fs.renameSync(filename, `${filename}.old`);
      write(filename, "其他程序新增的命令");
    }
  } });
  expect(result.status).toBe(0);
  for (const filename of aliases) expect(fs.readFileSync(filename, "utf8")).toBe("其他程序新增的命令");
});

test.skipIf(process.platform === "win32")("POSIX 套件移除後仍清理失效的命令 symlink", () => {
  const f = fixture();
  const aliases = aliasFiles(f, "freecd");
  const result = runWrapper(f, ["uninstall"], { onRemove: () => fs.unlinkSync(path.join(f.current, "bin/freeclaude.cjs")) });
  expect(result.status).toBe(0);
  for (const filename of aliases) expect(() => fs.lstatSync(filename)).toThrow();
});

test("無法確認全域歸屬時清理後顯示手動移除命令", () => {
  const result = runWrapper(fixture(), ["uninstall"], { queryStatus: 1 });
  expect(result.status).toBe(1);
  expect(result.calls.some((call) => call.args[0] === "remove")).toBe(false);
  expect(result.errors.join("\n")).toContain("無法確認 Bun 全域安裝位置");
});

test("損壞的全域設定不觸發套件移除", () => {
  const f = fixture({ config: "toml" });
  write(path.join(f.home, ".bunfig.toml"), "[install\n");
  const result = runWrapper(f, ["uninstall"]);
  expect(result.status).toBe(1);
  expect(result.calls.map((call) => call.args[0])).toEqual(["uninstall"]);
});

test("uninstall --help 不清除套件", () => {
  const result = runWrapper(fixture(), ["uninstall", "--help"]);
  expect(result.calls.map((call) => call.args[0])).toEqual(["uninstall"]);
});

test("purge、dashboard 與 restart 保留既有轉送行為", () => {
  const f = fixture();
  expect(runWrapper(f, ["purge"]).calls[0].args).toEqual(["purge", "--yes"]);
  expect(runWrapper(f, ["dashboard"]).calls[0].args).toEqual(["configure"]);
  expect(runWrapper(f, ["restart"]).calls.map((call) => call.args)).toEqual([["stop"], ["start"]]);
  expect(runWrapper(f, ["restart"], { nativeStatus: 3 }).calls).toHaveLength(1);
});

test("缺少平台套件或 binary 時回報 Bun 修復方式", () => {
  const f = fixture();
  const missingPackage = runWrapper(f, ["status"], { missingPackage: true });
  expect(missingPackage.status).toBe(1);
  expect(missingPackage.errors[0]).toContain(`bun add -g ${packageName}`);
  fs.unlinkSync(path.join(f.nativeDirectory, "bin", f.platform === "win32" ? "freeclaude.exe" : "freeclaude"));
  expect(runWrapper(f, ["status"]).errors[0]).toContain("找不到管理程式");
});

test("主套件 tarball 包含兩個可執行的 Bun 別名與解除安裝模組", () => {
  const root = temporaryDirectory();
  const result = spawnSync(process.execPath, ["pm", "pack", "--destination", root], { cwd: new URL(".", packageDirectory), encoding: "utf8", env: { ...process.env, BUN_TMPDIR: root } });
  expect(result.status, result.stderr).toBe(0);
  const archive = path.join(root, `mushroomtw-freeclaudedesktop-${manifest.version}.tgz`);
  expect(verifyPackage(archive).bin).toEqual(manifest.bin);
  expect(readArchive(archive).has("package/scripts/preuninstall.cjs")).toBe(false);
  const publish = spawnSync(process.execPath, ["publish", archive, "--access", "public", "--dry-run"], { encoding: "utf8", env: { ...process.env, NPM_CONFIG_TOKEN: "dry-run-placeholder", BUN_TMPDIR: root } });
  expect(publish.status, publish.stderr).toBe(0);
  expect(publish.stdout).toContain("(dry-run)");
});

test("沒有 Node.js 的 PATH 也能執行 Bun 包裝器", () => {
  const root = temporaryDirectory();
  const result = spawnSync(process.execPath, [fileURLToPath(new URL("bin/freeclaude.cjs", packageDirectory)), "--help"], { encoding: "utf8", env: { ...process.env, PATH: root } });
  expect(result.status, result.stderr).toBe(0);
  expect(result.stdout).toContain("FreeClaudeDesktop Bun 管理工具");
});

test.each(Object.entries(platforms))("平台封裝與 tarball 權限驗證：%s", (target, platform) => {
  const root = temporaryDirectory();
  write(path.join(root, "package.json"), '{"private":true}');
  write(path.join(root, "packages/freeclaudedesktop/package.json"), JSON.stringify(manifest));
  const extension = platform.os[0] === "win32" ? ".exe" : "";
  for (const name of ["freeclaude", "freeclaude-proxy"]) {
    const binary = path.join(root, "target/release", name + extension);
    write(binary, "發行套件測試替身");
    fs.chmodSync(binary, 0o755);
  }
  const build = spawnSync(process.execPath, [fileURLToPath(new URL("../scripts/pack-platform.mjs", import.meta.url)), target], { cwd: root, encoding: "utf8" });
  expect(build.status, build.stderr).toBe(0);
  const directory = path.join(root, "dist/npm", `freeclaudedesktop-${platform.suffix}`);
  const pack = spawnSync(process.execPath, ["pm", "pack", "--destination", ".."], { cwd: directory, encoding: "utf8", env: { ...process.env, BUN_TMPDIR: root } });
  expect(pack.status, pack.stderr).toBe(0);
  const archive = path.join(root, "dist/npm", `mushroomtw-freeclaudedesktop-${platform.suffix}-${manifest.version}.tgz`);
  if (process.platform === "win32" && !extension) {
    // Windows 檔案系統不能設定 POSIX 執行位元；拒絕這類跨平台產物，避免發行不可執行的 binary。
    const packedManifest = JSON.parse(readArchive(archive).get("package/package.json").content.toString());
    expect(packedManifest.name).toBe(`${packageName}-${platform.suffix}`);
    expect(() => verifyPackage(archive, target)).toThrow("缺少執行權限");
    return;
  }
  expect(verifyPackage(archive, target).name).toBe(`${packageName}-${platform.suffix}`);
});

test("隔離 Bun 全域安裝：兩個別名無需 Node，清理後實際移除套件與別名", async () => {
  const f = fixture();
  f.env = { ...process.env, ...f.env, HOME: f.home, USERPROFILE: f.home, BUN_INSTALL: path.join(f.root, "bun"), BUN_INSTALL_CACHE_DIR: path.join(f.root, "cache"), BUN_TMPDIR: path.join(f.root, "tmp") };
  fs.mkdirSync(f.env.BUN_TMPDIR, { recursive: true });
  const source = path.join(f.root, "package-source");
  write(path.join(source, "package.json"), JSON.stringify({ ...manifest, optionalDependencies: {} }));
  write(path.join(source, "bin/freeclaude.cjs"), wrapperSource);
  write(path.join(source, "bin/uninstall.cjs"), uninstallSource);
  const pack = spawnSync(process.execPath, ["pm", "pack", "--destination", f.root], { cwd: source, env: f.env, encoding: "utf8" });
  expect(pack.status, pack.stderr).toBe(0);
  const archive = path.join(f.root, `mushroomtw-freeclaudedesktop-${manifest.version}.tgz`);
  fs.unlinkSync(path.join(f.current, "package.json"));
  write(path.join(f.globalDirectory, "package.json"), '{"private":true}');
  const install = spawnSync(process.execPath, ["add", "-g", archive, "--ignore-scripts", "--force", "--cache-dir", path.join(f.root, "cache")], { cwd: f.globalDirectory, env: f.env, encoding: "utf8" });
  expect(install.status, install.stderr).toBe(0);
  expect(fs.readFileSync(path.join(f.current, "bin/freeclaude.cjs"), "utf8"), install.stdout).toBe(wrapperSource);
  const runtime = path.join(f.root, "runtime");
  fs.mkdirSync(runtime);
  fs.copyFileSync(process.execPath, path.join(runtime, process.platform === "win32" ? "bun.exe" : "bun"));
  fs.chmodSync(path.join(runtime, process.platform === "win32" ? "bun.exe" : "bun"), 0o755);
  for (const alias of ["freeclaude", "freecd"]) {
    const command = path.join(f.binDirectory, alias + (process.platform === "win32" ? ".exe" : ""));
    expect(fs.existsSync(command), install.stdout).toBe(true);
    const help = spawnSync(command, ["--help"], { cwd: f.globalDirectory, env: { ...f.env, PATH: runtime }, encoding: "utf8" });
    expect(help.status, help.stderr).toBe(0);
    expect(help.stdout).toContain("FreeClaudeDesktop Bun 管理工具");
  }
  // 原生測試程式只記錄事件；不執行真實服務、設定或資料清理。
  const events = path.join(f.root, "events");
  const nativeSource = path.join(f.root, "native_fixture.rs");
  write(nativeSource, 'fn main() { assert_eq!(std::env::args().nth(1).as_deref(), Some("uninstall")); std::fs::write(std::env::var("FREECD_TEST_EVENTS").unwrap(), "cleanup").unwrap(); }');
  const nativeDirectory = path.join(f.globalDirectory, "node_modules", `${packageName}-${process.platform}-${process.arch}`);
  write(path.join(nativeDirectory, "package.json"), JSON.stringify({ name: `${packageName}-${process.platform}-${process.arch}`, version: manifest.version }));
  const native = path.join(nativeDirectory, "bin", process.platform === "win32" ? "freeclaude.exe" : "freeclaude");
  fs.mkdirSync(path.dirname(native), { recursive: true });
  const compile = spawnSync("rustc", [nativeSource, "-o", native], { encoding: "utf8" });
  expect(compile.status, compile.stderr).toBe(0);
  const alias = path.join(f.binDirectory, process.platform === "win32" ? "freecd.exe" : "freecd");
  const result = spawnSync(alias, ["uninstall"], { cwd: f.globalDirectory, env: { ...f.env, PATH: runtime, FREECD_TEST_EVENTS: events }, encoding: "utf8" });
  expect(result.status, result.stderr).toBe(0);
  expect(fs.readFileSync(events, "utf8")).toBe("cleanup");
  expect(fs.existsSync(f.current), result.stdout).toBe(false);
  for (const alias of ["freeclaude", "freecd"]) expect(fs.existsSync(path.join(f.binDirectory, alias + (process.platform === "win32" ? ".exe" : "")))).toBe(false);
  for (let attempt = 0; attempt < 100 && fs.readdirSync(f.binDirectory).some((name) => name.startsWith(".freecd-uninstall-")); attempt++) await Bun.sleep(50);
  expect(fs.readdirSync(f.binDirectory).some((name) => name.startsWith(".freecd-uninstall-"))).toBe(false);
  if (process.platform === "win32") for (const alias of ["freeclaude", "freecd"]) expect(fs.existsSync(path.join(f.binDirectory, `${alias}.bunx`))).toBe(false);
  expect(JSON.parse(fs.readFileSync(path.join(f.globalDirectory, "package.json"), "utf8")).dependencies?.[packageName]).toBeUndefined();
}, 60_000);
