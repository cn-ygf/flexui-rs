# 发布流程

当前目标版本：`0.1.0`。CI 按本阶段约定暂不补强，以下检查在本地三平台环境执行。

## 发布前检查

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo check -p flexui --example hello
./scripts/gui-smoke.sh
./scripts/win-test.sh
```

Linux 还需在带 X11 的机器运行 `cargo test -p flexui-linux` 与 `./scripts/gui-smoke.sh`；无显示器时脚本会使用 `xvfb-run`。

版本一致性由 `crates/flexui/tests/release_version.rs` 检查，C ABI 的 `flex_version()` 直接从包版本计算。修改版本时还需同步 macOS 示例应用的 `CFBundleShortVersionString` 与 `CHANGELOG.md`。

## 产物检查

```bash
cargo package -p flexui-gfx --allow-dirty --no-verify
cargo build --release -p flexui-ffi
./scripts/build-macos-app.sh
```

`flexui-examples` 与 `flexui-gallery` 是工作区应用，已标记为不发布到 crates.io。

## 发布边界

仓库目前尚未声明开源许可证，因此不要执行 `cargo publish`。许可证属于项目所有者的法律选择；确定许可证并在各可发布包补齐对应元数据后，再按以下依赖拓扑发布 crate：

1. `flexui-resource`、`flexui-gfx`、`flexui-svg`
2. `flexui-i18n`
3. `flexui-core`
4. `flexui-xml`、`flexui-native-menu`
5. `flexui-macos`、`flexui-windows`、`flexui-linux`
6. `flexui`、`flexui-ffi`

在内部依赖尚未上传到 crates.io 前，门面包的 `cargo package -p flexui` 会因无法解析这些版本而失败，这是 Cargo 发布模型的预期结果。源码标签与 GitHub Release 也应由项目所有者在确认发布说明后创建。
