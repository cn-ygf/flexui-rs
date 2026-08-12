use std::path::PathBuf;

fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = crate_dir
        .join("../flexui-windows/flexui.manifest");
    let icon = crate_dir.join("assets/app.ico");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", icon.display());

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = icon.to_string_lossy();
        let manifest = manifest.to_string_lossy();
        let mut resources = winresource::WindowsResource::new();
        resources
            .set_icon(&icon)
            .set_manifest_file(&manifest);
        resources.compile().expect("编译 Windows 图标资源失败");
    }
}
