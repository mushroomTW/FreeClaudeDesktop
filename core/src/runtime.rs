/// 建立適合常駐 I/O 服務的小型執行緒池。
/// 保留 Tokio 原有的 TOKIO_WORKER_THREADS 環境變數設定能力。
pub fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    if std::env::var_os("TOKIO_WORKER_THREADS").is_none() {
        let workers = std::thread::available_parallelism()
            .map(|count| count.get().min(2))
            .unwrap_or(1);
        builder.worker_threads(workers);
    }
    builder.enable_all().build()
}
