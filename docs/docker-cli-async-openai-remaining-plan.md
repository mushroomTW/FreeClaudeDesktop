# Docker、CLI 與 async-openai 未完成實作計畫

本文件只列出目前尚未完成，或尚未完成實機驗證的工作。已完成的 Proxy、CLI 基礎指令、Admin API、Web Admin、companion RPC、native runtime、Docker 基礎映像與自動啟動不在此重複列出。

## 1. 原生執行檔安全更新

- [ ] 定義 release manifest 格式：版本、各平台資產名稱、SHA-256、下載 URL 與簽章資訊。
- [ ] 在 `freeclaude update --check` 顯示可用資產、目前版本、目標版本與 release URL。
- [ ] 實作下載到暫存檔，並在替換前驗證 SHA-256。
- [ ] 實作 Windows 的 helper process，避免直接覆蓋執行中的 `.exe`。
- [ ] 實作 macOS/Linux 的原子替換與舊檔回復策略。
- [ ] 替換後重新啟動 proxy、等待 `/healthz`，失敗時回復舊版。
- [ ] 保留 Settings、keyring 祕密與 autostart 設定，不隨更新刪除。
- [ ] 針對 checksum 錯誤、網路中斷、版本不相容與回復流程撰寫測試。

## 2. Docker runtime 完整生命週期

- [ ] 讓 Docker Compose 設定持久化資料卷，避免 Admin 設定在 container 重建後遺失。
- [ ] 確認容器內 keyring 不可用時的祕密保存策略，不將 API key 寫入 image layer 或 Git。
- [ ] `install/start/update --runtime docker` 後輪詢本機 `/healthz`，超時時輸出 container log 並回報失敗。
- [ ] `status --runtime docker` 解析 Compose 狀態與 health status，而非只回傳原始 JSON 字串。
- [ ] `update --runtime docker` 定義來源：本機重建或指定 registry image；兩者不可混淆。
- [ ] 補上 Docker daemon 可用、Compose 不存在、連接埠被占用與 healthcheck 失敗的整合測試。
- [ ] 在 Windows、macOS 與 Linux 實機驗證 localhost 綁定、non-root container 與 uninstall 行為。

## 3. companion WebSocket 協定與客戶端

- [ ] 寫出 handshake、request、response、error 與 timeout 的 JSON schema。
- [ ] 建立 companion client 模組，提供 request ID 配對、逾時、斷線重連與指數退避。
- [ ] 確認 token 僅用於 localhost 連線，且不寫入 log、錯誤回應或診斷輸出。
- [ ] 為 `DetectClaude`、`ApplySettings`、`RestoreSettings`、`LaunchClaude`、`GetStatus` 補 mock WebSocket 測試。
- [ ] 定義非文字 WebSocket 訊息、無效 JSON、未知欄位與關閉訊息的行為。

## 4. Web Admin 功能與可近用性

- [ ] 顯示模型清單、模型 alias、能力與 thinking/reasoning 設定。
- [ ] 提供 Extensions、Skills、Web tool 與 optimization 的設定介面；API 必須維持 allowlist。
- [ ] 補上 loading、成功/失敗 toast、欄位驗證與可讀的錯誤訊息。
- [ ] 改善鍵盤操作、focus 順序、label、色彩對比與窄螢幕配置。
- [ ] 以瀏覽器自動化或端對端測試驗證 API key 永不被回填或顯示。

## 5. async-openai 與 Claude 相容性驗證

- [ ] 用 mock OpenAI-compatible gateway 驗證 models、chat completion、streaming 與錯誤正規化。
- [ ] 驗證 tool call、tool result、thinking/reasoning block 與 SSE 結束事件不會被轉換遺失。
- [ ] 針對 LiteLLM `anthropic_messages` 路徑保留既有 header 與 SSE 行為，避免被 async-openai 路徑取代。
- [ ] 依 Claude Desktop 實際 wire format 完成 request/response/header 相容測試。

## 6. 跨平台整合與安全驗收

- [ ] Windows：驗證 Task Scheduler、CLI/proxy 同層部署、設定還原與 uninstall。
- [ ] macOS：驗證 LaunchAgent、Universal binary、權限與設定還原。
- [ ] Linux：驗證 systemd user service、XDG 設定路徑與 Docker Compose。
- [ ] 檢查所有 API 與 WebSocket 僅綁定 loopback；確認 token、API key、cookie 不會出現在 log。
- [ ] 執行 `cargo fmt --all --check`、`cargo test --all-targets`、`cargo clippy --all-targets -- -D warnings`，並記錄各平台結果。

## 完成定義

所有核取項完成後，需在至少一個支援平台完成 native 與 Docker 的端對端驗證；跨平台項目則應附上對應平台的實機驗證紀錄或明確的待驗證原因。
