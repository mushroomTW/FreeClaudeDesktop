# Docker 執行指南

本文件說明如何以 Docker Compose 執行 FreeClaudeDesktop Proxy。Docker runtime 是可選項；一般使用情境建議直接使用原生 runtime。

## 前置需求

- Docker Engine 或 Docker Desktop（包含 Docker Compose v2）
- 專案根目錄中的 `compose.yaml`、`Dockerfile` 與 `.env.example`

## 快速啟動

在專案根目錄執行：

```bash
docker compose up --build
```

Proxy 僅對本機公開：`http://127.0.0.1:3000`。Web Admin 不具有登入機制，請勿修改 `compose.yaml` 將連接埠暴露到區域網路或網際網路。

停止服務：

```bash
docker compose down
```

## 記憶體上限

預設上限為 **4 GB**。將 `.env.example` 複製為 `.env`，並設定 `FREECLAUDE_DOCKER_MEMORY_LIMIT`：

```dotenv
FREECLAUDE_DOCKER_MEMORY_LIMIT=2g
```

Docker Compose 支援如 `512m`、`1g`、`2g` 的記憶體單位。修改後重新建立容器：

```bash
docker compose up --build --force-recreate
```

## 以 CLI 管理 Docker runtime

在專案根目錄使用：

```bash
freeclaude install --runtime docker
freeclaude status --runtime docker
freeclaude stop --runtime docker
freeclaude start --runtime docker
freeclaude update --runtime docker
freeclaude uninstall --runtime docker --yes --purge-image
```

若 Compose 檔位於其他位置，請設定 `FREECLAUDE_COMPOSE_FILE` 為該檔案的絕對路徑。

## Companion Daemon

Docker 容器只包含 Proxy。CLI 每次啟動 Docker Proxy 時，都會在宿主機啟動 Companion Daemon，維持本機 `/companion` WebSocket 連線以供 Claude Desktop RPC 使用。
