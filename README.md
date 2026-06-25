# FreeClaudeLauncher

Rust 原生 Windows 桌面程式，背景執行本機 API proxy，並提供系統匣常駐。

## 建置

```bat
cargo build --release --bin FreeClaudeLauncher
```

成品：

```text
target\release\FreeClaudeLauncher.exe
```

## 開發啟動

```bat
run.bat
```

或直接執行：

```text
target\release\FreeClaudeLauncher.exe
```

## 檢查

```bat
cargo test
```
