//! 发布版本一致性测试：防止工作区 crate、依赖约束和门面版本发生漂移。

use std::path::Path;
use std::process::Command;

#[test]
fn 工作区包版本与门面版本一致() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(&workspace)
        .output()
        .expect("应能执行 cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata 失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected = env!("CARGO_PKG_VERSION");
    let mismatches = metadata["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|package| {
            let name = package["name"].as_str()?;
            let version = package["version"].as_str()?;
            (version != expected).then(|| format!("{name}={version}"))
        })
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "工作区包版本应统一为 {expected}，但发现：{}",
        mismatches.join(", ")
    );

    let bundle_script = std::fs::read_to_string(workspace.join("scripts/build-macos-app.sh"))
        .expect("应能读取 macOS 打包脚本");
    assert!(
        bundle_script.contains(&format!("<string>{expected}</string>")),
        "macOS CFBundleShortVersionString 应同步为 {expected}"
    );
    let changelog =
        std::fs::read_to_string(workspace.join("CHANGELOG.md")).expect("应能读取变更日志");
    assert!(
        changelog.contains(&format!("## [{expected}]")),
        "CHANGELOG 应包含 {expected} 版本节"
    );
}
