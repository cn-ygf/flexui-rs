//! 守护 `include/flexui.h` 与 Rust 源同步：用 cbindgen 依 `cbindgen.toml` 现场生成
//! 头文件，和已提交的版本逐字节比较。改了任何 `pub extern "C"` / `#[repr(C)]` /
//! 常量后忘了重新生成头文件，此测试即失败。
//!
//! 重新生成（改完接口后跑一次）：
//!   FLEXUI_BLESS_HEADER=1 cargo test -p flexui-ffi --test header_up_to_date

use std::path::PathBuf;

#[test]
fn header_up_to_date() {
    let crate_dir = env!("CARGO_MANIFEST_DIR");
    let config = cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml"))
        .expect("读取 cbindgen.toml 失败");

    let mut generated: Vec<u8> = Vec::new();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen 生成头文件失败")
        .write(&mut generated);
    let generated = String::from_utf8(generated).expect("头文件非 UTF-8");

    let header_path = PathBuf::from(crate_dir).join("include/flexui.h");

    // bless 模式：重新写出头文件（改完接口后用）。
    if std::env::var_os("FLEXUI_BLESS_HEADER").is_some() {
        std::fs::write(&header_path, generated.as_bytes()).expect("写 include/flexui.h 失败");
        return;
    }

    let committed = std::fs::read_to_string(&header_path).unwrap_or_default();
    assert_eq!(
        generated, committed,
        "include/flexui.h 与源码不同步；请运行：\n  \
         FLEXUI_BLESS_HEADER=1 cargo test -p flexui-ffi --test header_up_to_date\n\
         重新生成并提交头文件。"
    );
}
