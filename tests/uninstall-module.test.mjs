import { afterAll, expect, test } from "bun:test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";

// 以真實的 node:fs / node:os / node:child_process 載入解除安裝模組，
// 只在暫存目錄中建立全域安裝的目錄結構，驗證各分支與別名清理。
const require = createRequire(import.meta.url);
const { globalInstallation, cleanupGlobalAliases } = require("../packages/freeclaudedesktop/bin/uninstall.cjs");

const roots = [];

function temporaryRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "freecd-uninstall-test-"));
  roots.push(root);
  return root;
}

function withGlobalDir(value, run) {
  const previous = process.env.BUN_INSTALL_GLOBAL_DIR;
  if (value === undefined) delete process.env.BUN_INSTALL_GLOBAL_DIR;
  else process.env.BUN_INSTALL_GLOBAL_DIR = value;
  try {
    return run();
  } finally {
    if (previous === undefined) delete process.env.BUN_INSTALL_GLOBAL_DIR;
    else process.env.BUN_INSTALL_GLOBAL_DIR = previous;
  }
}

function createGlobalFixture(root, manifest = { dependencies: { "@mushroomtw/freeclaudedesktop": "1.0.0" } }) {
  const globalDir = path.join(root, "global");
  const packageDirectory = path.join(globalDir, "node_modules", "@mushroomtw", "freeclaudedesktop");
  const binDirectory = path.join(packageDirectory, "bin");
  fs.mkdirSync(binDirectory, { recursive: true });
  fs.writeFileSync(path.join(globalDir, "package.json"), JSON.stringify(manifest));
  // 真實安裝一定帶有入口檔；別名比對會用它確認是否屬於本套件。
  fs.writeFileSync(path.join(binDirectory, "freeclaude.cjs"), "#!/usr/bin/env bun\n");
  return { globalDir, packageDirectory, binDirectory };
}

afterAll(() => {
  for (const root of roots) {
    // Windows 上延後清理的子程序可能仍持有暫存檔，清理失敗不影響測試結果。
    try {
      fs.rmSync(root, { recursive: true, force: true });
    } catch {
      // 交由作業系統的暫存目錄清理。
    }
  }
});

test("未安裝全域套件時回報本機安裝", () => {
  const root = temporaryRoot();
  const globalDir = path.join(root, "global");
  fs.mkdirSync(globalDir, { recursive: true });
  const binDirectory = path.join(root, "elsewhere", "bin");
  fs.mkdirSync(binDirectory, { recursive: true });

  withGlobalDir(globalDir, () => {
    expect(globalInstallation(binDirectory)).toEqual({ kind: "local" });
  });
});

test("全域設定為相對路徑時回報 unknown", () => {
  withGlobalDir("relative/global", () => {
    const result = globalInstallation(process.cwd());
    expect(result.kind).toBe("unknown");
    expect(result.reason).toBe("Global directory uses a relative path");
  });
});

test("全域設定為無效路徑時回報 unknown", () => {
  const root = temporaryRoot();
  // 無法解析的設定會讓 absoluteDirectory 直接拒絕，而不是當成未設定。
  withGlobalDir(" ", () => {
    const result = globalInstallation(path.join(root, "bin"));
    expect(result.kind).toBe("unknown");
    expect(result.reason).toBe("Global directory uses a relative path");
  });
});

test("bin 目錄不在全域安裝位置時回報本機安裝", () => {
  const root = temporaryRoot();
  const { globalDir } = createGlobalFixture(root);
  const otherBin = path.join(root, "other", "bin");
  fs.mkdirSync(otherBin, { recursive: true });

  withGlobalDir(globalDir, () => {
    expect(globalInstallation(otherBin)).toEqual({ kind: "local" });
  });
});

test("全域 manifest 未包含本套件時回報 unknown", () => {
  const root = temporaryRoot();
  const { globalDir, binDirectory } = createGlobalFixture(root, { dependencies: {} });

  withGlobalDir(globalDir, () => {
    const result = globalInstallation(binDirectory);
    expect(result.kind).toBe("unknown");
    expect(result.reason).toBe("The global package manifest does not include the current package");
  });
});

test("全域安裝時回報安裝根目錄與別名清單", () => {
  const root = temporaryRoot();
  const { globalDir, binDirectory } = createGlobalFixture(root);

  withGlobalDir(globalDir, () => {
    const result = globalInstallation(binDirectory);
    expect(result.kind).toBe("global");
    expect(result.directory).toBe(fs.realpathSync(globalDir));
    expect(path.isAbsolute(result.binDirectory)).toBe(true);
    expect(Array.isArray(result.aliases)).toBe(true);
  });
});

test("身分相符的別名會被移除，其餘略過", () => {
  const root = temporaryRoot();
  const binDirectory = path.join(root, "bin");
  fs.mkdirSync(binDirectory, { recursive: true });
  const owned = path.join(binDirectory, "freeclaude");
  const stale = path.join(binDirectory, "freecd");
  const missing = path.join(binDirectory, "gone");
  fs.writeFileSync(owned, "owned");
  fs.writeFileSync(stale, "stale");

  const actual = fs.lstatSync(stale, { bigint: true });
  cleanupGlobalAliases({
    aliases: [
      { filename: owned, identity: fs.lstatSync(owned, { bigint: true }) },
      // 身分不符（例如已被其他安裝取代）時不得刪除。
      { filename: stale, identity: { dev: actual.dev, ino: actual.ino + 1n } },
      // 檔案已不存在（ENOENT）時略過。
      { filename: missing, identity: fs.lstatSync(owned, { bigint: true }) }
    ]
  });

  expect(fs.existsSync(owned)).toBe(false);
  expect(fs.existsSync(stale)).toBe(true);
});

test.skipIf(process.platform !== "win32")(
  "Windows 上被鎖定的 .exe 別名改以重新命名後延後清理",
  () => {
    const root = temporaryRoot();
    const binDirectory = path.join(root, "bin");
    fs.mkdirSync(binDirectory, { recursive: true });
    // 目錄無法用 unlink 移除，會走 Windows 專屬的「重新命名後由子程序清理」路徑。
    const locked = path.join(binDirectory, "freecd.exe");
    fs.mkdirSync(locked);

    cleanupGlobalAliases({
      aliases: [{ filename: locked, identity: fs.lstatSync(locked, { bigint: true }) }]
    });

    expect(fs.existsSync(locked)).toBe(false);
  }
);
