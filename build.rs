fn main() {
    println!("cargo:rerun-if-changed=resources/windows/velora.rc");
    println!("cargo:rerun-if-changed=assets/icon/velora.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("resources/windows/velora.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile velora Windows resources");
    }
}
