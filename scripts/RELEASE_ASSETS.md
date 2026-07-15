# Release 資產規格

每個 GitHub Release 都必須包含下列資產，否則快速開始 URL 無法取得對應平台的 binary：

- `install.sh` 與 `install.ps1`
- `checksums.txt`：每行格式為 `<sha256>  <filename>`
- `freeclaude-x86_64-unknown-linux-gnu.tar.gz`
- `freeclaude-aarch64-unknown-linux-gnu.tar.gz`
- `freeclaude-x86_64-apple-darwin.tar.gz`
- `freeclaude-aarch64-apple-darwin.tar.gz`
- `freeclaude-x86_64-pc-windows-msvc.zip`
- `freeclaude-aarch64-pc-windows-msvc.zip`

Unix tarball 的根目錄必須包含 `freeclaude` 與 `freeclaude-proxy`。Windows zip 的根目錄也必須包含 `freeclaude.exe` 與 `freeclaude-proxy.exe`。所有 archive 都必須計入 `checksums.txt`。

## 自動發行

推送 `v*` 格式的 Git 標籤會觸發 `.github/workflows/release.yml`。流程在六個對應平台的 GitHub-hosted runner 原生編譯、打包、產生 `checksums.txt`，最後建立同名 GitHub Release 並上傳所有資產。

手動觸發 Release 工作流程只會建置並保留 Actions artifact，方便在正式標籤前驗證；它不會建立 GitHub Release。`.github/workflows/ci.yml` 則會在 `main` 的推送與 Pull Request 上執行格式、Clippy 與測試。
