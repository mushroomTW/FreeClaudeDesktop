# FreeClaudeDesktop

以 pnpm 安裝及管理 FreeClaudeDesktop：

```sh
pnpm add -g @mushroomtw/freeclaudedesktop
freeclaude-proxy start
```

Web Admin 位於 `http://127.0.0.1:3000/admin`。移除套件時會停止服務、還原 Claude 設定並清除 FreeClaudeDesktop 所擁有的本機資料：

```sh
pnpm remove -g @mushroomtw/freeclaudedesktop
```
