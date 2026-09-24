fn main() {
    println!("cargo:rerun-if-changed=resources/windows/maksher.rc");
    println!("cargo:rerun-if-changed=assets/icon/maksher.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("resources/windows/maksher.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to compile maksher Windows resources");
    }
}
