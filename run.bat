@echo off
title FreeClaudeLauncher Rust (API Proxy)
chcp 65001 > nul

echo ==================================================
echo         Free Claude Desktop Proxy Launcher (Rust)
echo ==================================================
echo.
echo Starting native launcher...
echo Local API proxy: http://127.0.0.1:3000/v1/messages
echo.
echo ==================================================
echo * Development wrapper only
echo * Closing this window stops cargo run
echo ==================================================

cargo run --release --bin FreeClaudeLauncher

pause
