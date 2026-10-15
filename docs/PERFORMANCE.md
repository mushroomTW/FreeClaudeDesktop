# 常駐資源優化

## 執行方式

CLI、Companion daemon、獨立 Proxy 與背景 Proxy 共用 `core/src/runtime.rs` 的 Tokio 建構函式。預設工作執行緒數為可用邏輯核心數與 2 的較小值；仍可透過 `TOKIO_WORKER_THREADS` 指定數量。這只限制非同步工作執行緒，不限制 Tokio 按需建立的 blocking 執行緒。

Proxy 的非阻塞診斷日誌佇列由套件預設的 128,000 槽縮至 2,048 槽，以減少啟動時的固定配置。保留原本的 lossy 模式：磁碟寫入落後且佇列已滿時會丟棄診斷日誌。`api-calls.log` 使用另一條寫入路徑，不受這個佇列大小影響。

## 串流生命週期

- 下游接收器關閉時，取消整個轉換 future，釋放等待中的上游 HTTP 回應與背景工作。
- 收到 `[DONE]` 後結束轉換，不再等待上游連線 EOF；仍保留 finish reason 後、DONE 前的 usage 事件。
- 以位元組緩衝區保存未完成的行，完整一行才解碼 UTF-8，避免中文字元跨網路區塊時損壞。
- 每個網路區塊僅搬移一次未完成的尾段，省去逐行複製剩餘緩衝區的成本。
- 緩衝配置超過 64 KiB、未完成尾段已降至 64 KiB 以下時，改用僅容納尾段的新配置，釋放大型事件留下的尖峰容量。一般區塊重用原配置，大型未完成行也不反覆縮容。

## 本機量測

2026-10-15，Windows、6 邏輯核心、debug 執行檔，移除 `TOKIO_WORKER_THREADS` 覆寫。使用獨立暫存設定目錄與隨機 loopback 連接埠，啟動後等待 2 秒，Proxy 執行 100 次 `/healthz`，再以 psutil 量測 2 秒閒置 CPU 與程序記憶體。Companion 使用未設定 token 的重連狀態，並非已連線的實際使用情境。每個版本僅量測一次。

| 指標 | 優化前 | 優化後 |
| --- | ---: | ---: |
| Proxy 程序執行緒 | 11 | 7 |
| Proxy RSS | 19,988,480 bytes | 15,761,408 bytes |
| Proxy private bytes | 7,536,640 bytes | 3,198,976 bytes |
| Companion 程序執行緒 | 10 | 6 |
| Companion RSS | 12,292,096 bytes | 12,214,272 bytes |
| Companion private bytes | 2,658,304 bytes | 2,420,736 bytes |
| 兩個程序各自的閒置 CPU | 0.0% | 0.0% |

Proxy RSS 在這次量測下降約 21%，private bytes 下降約 58%。健康檢查耗時波動較大，未據此宣稱吞吐量或延遲改善。以上結果不代表 release 版本、長時間負載、真實模型串流或其他平台的占用；高併發部署可調整工作執行緒數後重新量測。

## 驗證

```text
cargo check --workspace
cargo test --workspace
```

完整測試結果：208 通過、1 略過。新增的回歸測試涵蓋下游取消、DONE 後上游保持開啟、UTF-8 跨區塊與大量同區塊事件，以及大型行處理後的容量回收、尾段保留與一般配置重用。生命週期測試在修正前因等待逾時失敗；容量回收測試在修正前因保留尖峰配置失敗。
