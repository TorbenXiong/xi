fn main() {
    let runtime = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("runtime")
        .join(
            if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
                "xi-agent-cli.exe"
            } else {
                "xi-agent-cli"
            },
        );
    println!("cargo:rerun-if-changed={}", runtime.display());
    if std::env::var("PROFILE").as_deref() == Ok("release") && !runtime.is_file() {
        panic!("发布构建缺少 Runtime，请从仓库根目录运行 node scripts/build-desktop.mjs");
    }
    tauri_build::build()
}
