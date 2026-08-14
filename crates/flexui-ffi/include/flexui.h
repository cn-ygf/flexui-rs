/* flexui-rs C ABI 头文件（手写；可用 cbindgen 依 cbindgen.toml 重新生成）。 */
#ifndef FLEXUI_H
#define FLEXUI_H

#ifdef __cplusplus
extern "C" {
#endif

/* ===== 基础 ===== */

/* 库版本。 */
unsigned int flex_version(void);

/* 校验并构建一段 XML，返回根节点顶层子节点数；出错返回负数。
 * -1 空指针/编码错误；-2 解析/构建失败；-3 内部 panic。 */
int flex_load_check(const char *xml);

/* ===== 运行 ===== */

/* 点击回调类型：name 为按钮 name，user 为注册时传入的用户数据。 */
typedef void (*FlexClickCb)(const char *name, void *user);

/* 注册全局点击回调（传 NULL 取消）。仅配合 flex_run_xml 使用。 */
void flex_set_click_callback(FlexClickCb cb, void *user);

/* 用 XML 启动应用并进入主事件循环（阻塞）；有 name 的按钮点击回调到 flex_set_click_callback。
 * 0 成功，负数为错误码。 */
int flex_run_xml(const char *title, int width, int height, const char *xml);

enum {
  FLEX_WINDOW_MINIMIZED = 1,
  FLEX_WINDOW_MAXIMIZED = 2,
  FLEX_WINDOW_RESTORED = 3
};

/* 窗口委托：各钩子为可空函数指针，ctx 仅在回调期间有效。 */
typedef struct FlexDelegate {
  void (*on_before_init)(void *ctx, void *user);
  void (*on_init)(void *ctx, void *user);
  void (*on_initialized)(void *ctx, void *user);
  void (*on_click)(const char *name, void *ctx, void *user);
  void (*on_context)(const char *name, float x, float y, void *ctx, void *user);
  void (*on_window_state)(int state, void *ctx, void *user);
  int  (*on_closing)(void *ctx, void *user); /* 返回非 0 允许关闭 */
  void (*on_closed)(void *user);
} FlexDelegate;

/* 用 XML + 委托启动（阻塞）。delegate 可为 NULL。0 成功，负数错误码。 */
int flex_run(const char *title, int width, int height, const char *xml,
             const FlexDelegate *delegate, void *user);

/* ===== 回调内的窗口上下文操作（FlexCtx*，仅回调期间有效）===== */

void flex_ctx_set_text(void *ctx, const char *name, const char *text);
/* 读文本到 out（含 NUL）；返回长度，未找到/出错 -1。 */
int  flex_ctx_get_text(void *ctx, const char *name, char *out, int out_len);
/* 1 选中 / 0 未选 / -1 未找到。 */
int  flex_ctx_is_selected(void *ctx, const char *name);
void flex_ctx_set_enabled(void *ctx, const char *name, int enabled);
void flex_ctx_set_title(void *ctx, const char *title);
void flex_ctx_close(void *ctx);

/* ===== 工作线程/异步任务投递到 UI 线程 ===== */

typedef struct FlexMainProxy FlexMainProxy;
typedef void (*FlexUiTaskFn)(void *ctx, void *user);

/* 在初始化回调中取得句柄；每个返回值都必须调用 flex_main_proxy_free。 */
FlexMainProxy *flex_ctx_main_proxy(void *ctx);
FlexMainProxy *flex_main_proxy_clone(const FlexMainProxy *proxy);
/* 1 表示任务已接受，0 表示窗口已关闭或参数无效。 */
int flex_main_proxy_post(const FlexMainProxy *proxy, FlexUiTaskFn task, void *user);
void flex_main_proxy_free(FlexMainProxy *proxy);

/* ===== 系统剪贴板 ===== */

/* 读剪贴板文本到 out（含 NUL）；返回长度，空/出错 -1。 */
int flex_clipboard_get_text(char *out, int out_len);
/* 写剪贴板文本。 */
void flex_clipboard_set_text(const char *text);

/* ===== 文件对话框 ===== */

/* filter_exts 为逗号分隔扩展名（如 "png,jpg"），各字段可为 NULL。 */
typedef struct FlexFileDialog {
  const char *title;
  const char *default_dir;
  const char *default_name;
  const char *filter_name;
  const char *filter_exts;
} FlexFileDialog;

/* 路径写入 out（含 NUL）；返回长度，取消/出错 -1。 */
int flex_dialog_open_file(const FlexFileDialog *opts, char *out, int out_len);
int flex_dialog_open_directory(const FlexFileDialog *opts, char *out, int out_len);
int flex_dialog_save_file(const FlexFileDialog *opts, char *out, int out_len);
int flex_dialog_save_directory(const FlexFileDialog *opts, char *out, int out_len);

#ifdef __cplusplus
}
#endif

#endif /* FLEXUI_H */
