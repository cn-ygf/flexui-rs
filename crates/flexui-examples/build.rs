use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zip::write::FileOptions;

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            let name = path.file_name().and_then(|name| name.to_str());
            if !matches!(name, Some(".DS_Store" | "app.icns" | "app.ico")) {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn embed_assets(crate_dir: &Path) {
    let assets_dir = crate_dir.join("assets");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo 必须提供 OUT_DIR"))
        .join("assets.zip");
    let mut files = Vec::new();
    collect_files(&assets_dir, &mut files).expect("扫描示例资源失败");
    files.sort();

    let mut archive = zip::ZipWriter::new(File::create(output).expect("创建内嵌资源包失败"));
    let options: FileOptions<()> = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for path in files {
        let name = path
            .strip_prefix(&assets_dir)
            .expect("资源必须位于 assets 目录")
            .to_string_lossy()
            .replace('\\', "/");
        archive.start_file(name, options).expect("写入资源包条目失败");
        let bytes = std::fs::read(&path).expect("读取示例资源失败");
        archive.write_all(&bytes).expect("写入示例资源失败");
    }
    archive.finish().expect("完成内嵌资源包失败");
    println!("cargo:rerun-if-changed={}", assets_dir.display());
}

fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    embed_assets(&crate_dir);
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
