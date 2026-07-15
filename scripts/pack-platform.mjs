import { cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";

const [target, outputDirectory = "dist/npm"] = process.argv.slice(2);
const platforms = {
  "aarch64-apple-darwin": { os: ["darwin"], cpu: ["arm64"], suffix: "darwin-arm64" },
  "x86_64-apple-darwin": { os: ["darwin"], cpu: ["x64"], suffix: "darwin-x64" },
  "x86_64-unknown-linux-gnu": { os: ["linux"], cpu: ["x64"], suffix: "linux-x64" },
  "aarch64-pc-windows-msvc": { os: ["win32"], cpu: ["arm64"], suffix: "win32-arm64" },
  "x86_64-pc-windows-msvc": { os: ["win32"], cpu: ["x64"], suffix: "win32-x64" }
};

const platform = platforms[target];
if (!platform) {
  throw new Error(`未知的 Rust target：${target}`);
}

const extension = target.includes("windows") ? ".exe" : "";
const sourceDirectory = "target/release";
const output = join(outputDirectory, `freeclaudedesktop-${platform.suffix}`);
rmSync(output, { recursive: true, force: true });
mkdirSync(join(output, "bin"), { recursive: true });

for (const name of ["freeclaude", "freeclaude-proxy"]) {
  const source = join(sourceDirectory, `${name}${extension}`);
  if (!existsSync(source)) throw new Error(`找不到發行 binary：${source}`);
  cpSync(source, join(output, "bin", basename(source)));
}

writeFileSync(join(output, "package.json"), `${JSON.stringify({
  name: `@mushroomtw/freeclaudedesktop-${platform.suffix}`,
  version: "0.1.1",
  description: `FreeClaudeDesktop ${platform.suffix} binary`,
  license: "MIT",
  os: platform.os,
  cpu: platform.cpu,
  files: ["bin"]
}, null, 2)}\n`);
