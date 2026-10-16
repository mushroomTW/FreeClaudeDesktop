# FreeClaudeDesktop

需要 Bun 1.4.3 以上版本，以 Bun 安裝及管理 FreeClaudeDesktop：

```sh
bun add -g @mushroomtw/freeclaudedesktop
freecd start
```

可使用 `freecd status`、`freecd restart`、`freecd dashboard` 與 `freecd path` 管理服務。Web 控制台預設位於 `http://127.0.0.1:3000/dashboard`。

## 解除安裝

全域安裝可一鍵清理本機狀態並移除套件：

```sh
freecd uninstall
```

清理成功後，Bun 包裝器會等待原生程式退出，再移除目前的 Bun 全域套件。清理失敗會保留套件；移除失敗時請重試 `bun remove -g @mushroomtw/freeclaudedesktop --ignore-scripts`。區域安裝與原生 binary 只清理應用程式狀態；無法確認全域安裝歸屬時，會顯示手動移除命令。`freecd purge` 會保留套件。

## 記憶體與效能

常駐 runtime 以單一 `freeclaude` 程序執行 Proxy 與 Companion。Proxy 逐段轉送上游串流，每個請求主體上限為 `16 MiB`；記憶體占用會隨同時處理的請求與串流數量而變化，沒有固定的 RAM 數值保證。每個 Tokio runtime 預設最多使用 `2` 個非同步工作執行緒，可用 `TOKIO_WORKER_THREADS` 調整。

## 從 npm 遷移

先用舊版程式清理本機狀態，再透過原本管理 npm 全域套件的流程移除舊命令；接著以 Bun 安裝：

```sh
freecd uninstall
bun add -g @mushroomtw/freeclaudedesktop
freecd install
```

清理會刪除應用程式資料，重新安裝後需再次設定 Gateway。
