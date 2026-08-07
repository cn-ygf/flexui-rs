//! FFI 冒烟（Rust 直调导出的 C ABI 函数）：可在 host 与交叉编译后的 wine 中运行，
//! 验证 flexui-ffi 的非阻塞入口在各平台一致。

fn main() {
    use std::ffi::CString;

    let xml = CString::new(
        "<VBox spacing=\"8\">\
           <Button name=\"ok\" text=\"OK\"/>\
           <Label text=\"hi\"/>\
         </VBox>",
    )
    .unwrap();

    let v = flexui_ffi::flex_version();
    let n = flexui_ffi::flex_load_check(xml.as_ptr());
    println!("flex_version={v} flex_load_check={n}");

    if v == 1 && n == 2 {
        println!("FFI-OK");
    } else {
        println!("FFI-FAIL");
        std::process::exit(1);
    }
}
