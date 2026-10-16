export const platforms = {
  "aarch64-apple-darwin": { os: ["darwin"], cpu: ["arm64"], suffix: "darwin-arm64" },
  "x86_64-apple-darwin": { os: ["darwin"], cpu: ["x64"], suffix: "darwin-x64" },
  "aarch64-unknown-linux-gnu": { os: ["linux"], cpu: ["arm64"], suffix: "linux-arm64" },
  "x86_64-unknown-linux-gnu": { os: ["linux"], cpu: ["x64"], suffix: "linux-x64" },
  "aarch64-pc-windows-msvc": { os: ["win32"], cpu: ["arm64"], suffix: "win32-arm64" },
  "x86_64-pc-windows-msvc": { os: ["win32"], cpu: ["x64"], suffix: "win32-x64" }
};
