# 一鍵安裝 Release 資產

每個 GitHub Release 必須上傳下列檔案，讓穩定安裝器 URL 可依平台下載並驗證 binary：

- `install.sh` 與 `install.ps1`
- `checksums.txt`（每個 archive 一行：`<sha256>  <filename>`）
- `freeclaude-x86_64-unknown-linux-gnu.tar.gz`
- `freeclaude-aarch64-unknown-linux-gnu.tar.gz`
- `freeclaude-x86_64-apple-darwin.tar.gz`
- `freeclaude-aarch64-apple-darwin.tar.gz`
- `freeclaude-x86_64-pc-windows-msvc.zip`
- `freeclaude-aarch64-pc-windows-msvc.zip`

Unix tarball 的根目錄必須包含 `freeclaude` 與 `freeclaude-proxy`；Windows zip 根目錄必須包含 `freeclaude.exe` 與 `freeclaude-proxy.exe`。上傳前以 SHA-256 建立 `checksums.txt`，並在每次 Release 一起上傳兩個安裝器腳本。
