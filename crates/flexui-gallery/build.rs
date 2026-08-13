use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zip::write::FileOptions;

fn collect_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else if path.file_name().and_then(|name| name.to_str()) != Some(".DS_Store") {
            files.push(path);
        }
    }
    Ok(())
}

fn main() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let assets_dir = crate_dir.join("assets");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo 必须提供 OUT_DIR"))
        .join("assets.zip");
    let mut files = Vec::new();
    collect_files(&assets_dir, &mut files).expect("扫描 Gallery 资源失败");
    files.sort();

    let mut archive = zip::ZipWriter::new(File::create(output).expect("创建资源包失败"));
    let options: FileOptions<()> = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for path in files {
        let name = path
            .strip_prefix(&assets_dir)
            .expect("资源必须位于 assets 目录")
            .to_string_lossy()
            .replace('\\', "/");
        archive.start_file(name, options).expect("写入资源条目失败");
        archive
            .write_all(&std::fs::read(&path).expect("读取资源失败"))
            .expect("写入资源失败");
    }
    archive.finish().expect("完成资源包失败");
    println!("cargo:rerun-if-changed={}", assets_dir.display());
}
