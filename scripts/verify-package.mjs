import { readFileSync } from "node:fs";
import { gunzipSync } from "node:zlib";
import { platforms } from "./platforms.mjs";
import { pathToFileURL } from "node:url";

// Bun 打包的標準 tar header；直接讀取模式可避免依賴各平台的 tar 輸出格式。
// 取出以 NUL 補滿的欄位內容（不使用正則，避免回溯風險）。
function headerField(header, start, length) {
  const raw = header.subarray(start, start + length).toString();
  const end = raw.indexOf("\0");
  return end === -1 ? raw : raw.slice(0, end);
}

export function readArchive(filename) {
  const tar = gunzipSync(readFileSync(filename));
  const files = new Map();
  for (let offset = 0; offset + 512 <= tar.length;) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const name = [headerField(header, 345, 155), headerField(header, 0, 100)].filter(Boolean).join("/");
    const size = Number.parseInt(headerField(header, 124, 12).trim(), 8);
    if (!Number.isSafeInteger(size) || size < 0 || offset + 512 + size > tar.length) throw new Error("無效的 tar 檔案大小");
    const type = headerField(header, 156, 1);
    if (type === "0" || type === "") {
      files.set(name, { mode: Number.parseInt(headerField(header, 100, 8).trim(), 8), content: tar.subarray(offset + 512, offset + 512 + size) });
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  return files;
}

// 平台專屬套件：檢查 metadata 是否對應目標 Rust target，以及 binary 是否存在且有執行權限。
function verifyPlatformPackage(manifest, source, target, required) {
  const platform = platforms[target];
  if (!platform) throw new Error(`未知的 Rust target：${target}`);
  if (manifest.name !== `${source.name}-${platform.suffix}` || JSON.stringify(manifest.os) !== JSON.stringify(platform.os) || JSON.stringify(manifest.cpu) !== JSON.stringify(platform.cpu)) {
    throw new Error("平台套件 metadata 不一致");
  }
  const extension = platform.os[0] === "win32" ? ".exe" : "";
  for (const name of ["freeclaude"]) {
    const file = required(`bin/${name}${extension}`);
    if (file.content.length === 0) throw new Error(`發行 binary 為空：${name}`);
    if (!extension && (file.mode & 0o111) === 0) throw new Error(`發行 binary 缺少執行權限：${name}`);
  }
}

// 主套件：檢查命令入口、相依設定，並確認舊的解除安裝掛鉤已移除。
function verifyMainPackage(manifest, source, files, required) {
  if (manifest.name !== source.name || manifest.bin?.freecd !== "bin/freeclaude.cjs" || manifest.bin?.freeclaude !== "bin/freeclaude.cjs") throw new Error("主套件命令入口不一致");
  if (manifest.engines?.bun !== source.engines.bun || JSON.stringify(manifest.optionalDependencies) !== JSON.stringify(source.optionalDependencies)) throw new Error("主套件相依設定不一致");
  const entry = required("bin/freeclaude.cjs");
  if (!entry.content.toString().startsWith("#!/usr/bin/env bun\n") || (entry.mode & 0o111) === 0) throw new Error("主套件缺少可執行的 Bun 入口");
  required("bin/uninstall.cjs");
  required("README.md");
  if (manifest.scripts?.preuninstall || files.has("package/scripts/preuninstall.cjs")) throw new Error("主套件仍包含舊解除安裝掛鉤");
}

export function verifyPackage(filename, target) {
  const files = readArchive(filename);
  const required = (name) => {
    const file = files.get(`package/${name}`);
    if (!file) throw new Error(`發行套件缺少 ${name}`);
    return file;
  };
  const manifest = JSON.parse(required("package.json").content.toString());
  const source = JSON.parse(readFileSync(new URL("../packages/freeclaudedesktop/package.json", import.meta.url), "utf8"));
  if (manifest.version !== source.version) throw new Error("發行套件版本不一致");
  if (target) {
    verifyPlatformPackage(manifest, source, target, required);
  } else {
    verifyMainPackage(manifest, source, files, required);
  }
  return manifest;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [filename, target] = process.argv.slice(2);
  if (!filename) throw new Error("用法：bun scripts/verify-package.mjs <tarball> [Rust target]");
  console.log(`已驗證 ${verifyPackage(filename, target).name}：${filename}`);
}
