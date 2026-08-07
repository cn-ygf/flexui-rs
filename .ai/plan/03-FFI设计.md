# flexui-rs C ABI / FFI 设计（预留）

需求 R6：未来暴露 C ABI 给别的语言（C/C++/Python/C#/Go…）调用。一期**不实现全量导出**，
但架构必须提前留口，避免后期返工。本文给出导出策略与约束。

## 1. 设计原则

- **薄封装**：`flexui-ffi`（L6）只是把 L5 的安全 Rust API 转成 C 可用形态，不含业务逻辑。
- **不透明句柄**：对外只给 `*mut FlexApp / *mut FlexWindow / *mut FlexWidget` 等不透明指针，
  隐藏 Rust 内部结构，保证 ABI 稳定。
- **零 panic 穿越边界**：每个 `extern "C"` 入口用 `catch_unwind` 包裹，panic 转错误码。
- **明确所有权**：谁创建谁释放；提供成对的 `flex_xxx_create` / `flex_xxx_destroy`。
- **C 兼容类型**：只用 `#[repr(C)]` 结构、原始指针、`c_int`、UTF-8 `*const c_char`、
  函数指针；不跨边界传 Rust `String/Vec/枚举默认布局`。

## 2. 导出形态（示意）

```c
typedef struct FlexApp    FlexApp;
typedef struct FlexWindow FlexWindow;
typedef struct FlexWidget FlexWidget;

// 生命周期
FlexApp*  flex_app_new(void);
void      flex_app_run(FlexApp*);            // 进主循环（阻塞）
void      flex_app_free(FlexApp*);

// 窗口
FlexWindow* flex_window_new(FlexApp*, const char* title, int w, int h);
void        flex_window_set_title(FlexWindow*, const char* utf8);

// 控件（Builder 的 C 化）
FlexWidget* flex_button_new(const char* text);
void        flex_widget_set_rect(FlexWidget*, float x, float y, float w, float h);
void        flex_window_set_root(FlexWindow*, FlexWidget*);

// 事件回调：C 函数指针 + 用户 data
typedef void (*FlexClickCb)(FlexWidget* self, void* user_data);
void flex_button_on_click(FlexWidget*, FlexClickCb cb, void* user_data);
```

对应的 Rust 侧：

```rust
#[repr(C)] pub struct FlexApp { /* 内部持有真正的 App */ }

#[no_mangle]
pub extern "C" fn flex_app_run(app: *mut FlexApp) {
    let _ = std::panic::catch_unwind(|| {
        // 解引用 + 调 L5 API
    });
}
```

## 3. 跨边界规则

| 事项 | 规则 |
| ---- | ---- |
| 字符串 | 入参 `*const c_char`（UTF-8, 调用方持有）；出参由库分配 + 提供 `flex_string_free` |
| 回调 | C 函数指针 + `void* user_data`；库负责在正确线程调用 |
| 错误 | 返回 `int` 错误码，或出参 `FlexStatus*`；不抛异常、不 panic 穿越 |
| 线程 | UI 调用必须在主线程；文档明确，或提供「投递到主线程」的 C 接口 |
| 枚举 | 用 `#[repr(C)] enum` 或整型常量，固定数值 |
| 内存 | 每个 `_new`/`_create` 配套 `_free`/`_destroy`，所有权文档化 |

## 4. 头文件生成

- 用 **cbindgen** 从 `flexui-ffi` 自动生成 `flexui.h`，纳入 CI 保证与代码同步。
- crate 类型设 `crate-type = ["cdylib", "staticlib"]`，同时产动态库/静态库。

## 5. 对上层 API 的反向约束（一期就要遵守）

为了 R6 顺利，L5 公共 API 设计时应：
- 核心对象能用**句柄/ID 语义**表达（不强依赖泛型和生命周期参数暴露给用户）。
- 事件回调用「`Fn` 闭包」在 Rust 侧，但保证能降级为「函数指针 + data」的 C 形态。
- 避免在公共类型里放无法 `#[repr(C)]` 化的结构，或为其准备 C 映射版本。

> 结论：一期只需**保证 L5 API 的可 C 化**，`flexui-ffi` crate 可先留空骨架，
> 待 Rust API 稳定后再补全导出。
