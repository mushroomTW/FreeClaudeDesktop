pub fn embed() {
    println!("cargo:rerun-if-changed=windows.rc");
    println!("cargo:rerun-if-changed=../icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    embed_resource::compile("windows.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("編譯 Windows 圖示資源失敗");
}
