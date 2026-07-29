# FreeClaudeDesktop

以 npm 安裝及管理 FreeClaudeDesktop：

```sh
npm install -g @mushroomtw/freeclaudedesktop
freecd start
```

可使用 `freecd status`、`freecd restart`、`freecd dashboard` 與 `freecd path` 管理服務。Web 控制台預設位於 `http://127.0.0.1:3000/dashboard`。

## 解除安裝

移除 npm 套件前，請先清理本機狀態：

```sh
freecd uninstall
npm uninstall -g @mushroomtw/freeclaudedesktop
```
