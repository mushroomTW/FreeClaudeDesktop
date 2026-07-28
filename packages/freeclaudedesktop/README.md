# FreeClaudeDesktop

以 npm 安裝及管理 FreeClaudeDesktop：

```sh
npm install -g @mushroomtw/freeclaudedesktop
freecd start
```

可使用 `freecd status`、`restart`、`dashboard`、`path` 與 `purge` 管理服務。Web 控制台預設位於 `http://127.0.0.1:3000/dashboard`。

移除套件時會停止服務、還原 Claude 設定並清除 FreeClaudeDesktop 所擁有的本機資料：

完整解除安裝前，請先執行：

```sh
freecd uninstall
npm uninstall -g @mushroomtw/freeclaudedesktop
```
