import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, join } from "node:path";
import { platforms } from "./platforms.mjs";

const [target, outputDirectory = "dist/npm"] = process.argv.slice(2);
const packageVersion = JSON.parse(
  readFileSync("packages/freeclaudedesktop/package.json", "utf8")
).version;

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
  version: packageVersion,
  description: `FreeClaudeDesktop ${platform.suffix} binary`,
  license: "MIT",
  os: platform.os,
  cpu: platform.cpu,
  files: ["bin"]
}, null, 2)}\n`);
