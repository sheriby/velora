fn main() {
    println!("cargo:rerun-if-changed=resources/windows/velora.rc");
    println!("cargo:rerun-if-changed=resources/windows/velora.manifest.xml");
    println!("cargo:rerun-if-changed=assets/icon/velora.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // velora.rc 内嵌的 Common-Controls v6 manifest 是运行时硬依赖
        // （TaskDialogIndirect 入口点；缺 manifest 时加载器绑定 comctl32 v5
        // 会直接拒绝启动），所以用 manifest_required 使丢失时构建即失败。
        embed_resource::compile("resources/windows/velora.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile velora Windows resources");
    }
}
