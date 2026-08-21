#ifndef FLEXUI_H
#define FLEXUI_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * C 侧窗口状态事件编号。
 */
#define FLEX_WINDOW_MINIMIZED 1

#define FLEX_WINDOW_MAXIMIZED 2

#define FLEX_WINDOW_RESTORED 3

/**
 * C 侧控件事件类型编号（用于 `FlexDelegate::on_control_event`）。
 * 各类型用到 on_control_event 的哪些参数：
 *   HOVER/PRESSED/FOCUS/SELECTED → i0=0/1；SELECTION → i0=索引(无=-1)；
 *   TEXT → text；VALUE → f0；SCROLL → f0=横偏移, f1=纵偏移。
 */
#define FLEX_CTRL_HOVER_CHANGED 1

#define FLEX_CTRL_PRESSED_CHANGED 2

#define FLEX_CTRL_FOCUS_CHANGED 3

#define FLEX_CTRL_TEXT_CHANGED 4

#define FLEX_CTRL_SELECTED_CHANGED 5

#define FLEX_CTRL_SELECTION_CHANGED 6

#define FLEX_CTRL_VALUE_CHANGED 7

#define FLEX_CTRL_SCROLL_CHANGED 8

/**
 * C 侧持有的不透明 UI 线程投递句柄。
 */
typedef struct FlexMainProxy FlexMainProxy;

/**
 * C 侧窗口委托：各钩子为可空函数指针，ctx 仅在回调期间有效。
 */
typedef struct {
  void (*on_before_init)(void *ctx, void *user);
  void (*on_init)(void *ctx, void *user);
  void (*on_initialized)(void *ctx, void *user);
  void (*on_click)(const char *name, void *ctx, void *user);
  /**
   * 通用控件事件（复选框勾选、滑块/进度值变化、文本变化、下拉选择、滚动等）。
   * `ev_type` 见 `FLEX_CTRL_*`；按类型读取 i0/f0/f1/text（其余为 0/NULL）。
   */
  void (*on_control_event)(const char *name,
                           int ev_type,
                           int i0,
                           float f0,
                           float f1,
                           const char *text,
                           void *ctx,
                           void *user);
  void (*on_double_click)(const char *name, void *ctx, void *user);
  void (*on_context)(const char *name, float x, float y, void *ctx, void *user);
  /**
   * 窗口尺寸变化（逻辑像素）。
   */
  void (*on_size)(float width, float height, void *ctx, void *user);
  /**
   * 按键（导航/功能键的平台无关键码）。
   */
  void (*on_key)(int key, void *ctx, void *user);
  void (*on_window_state)(int state, void *ctx, void *user);
  /**
   * 返回非 0 允许关闭，0 阻止关闭。
   */
  int (*on_closing)(void *ctx, void *user);
  void (*on_closed)(void *user);
} FlexDelegate;

/**
 * C 侧文件对话框配置。filter_exts 为逗号分隔扩展名（如 "png,jpg"），可空。
 */
typedef struct {
  const char *title;
  const char *default_dir;
  const char *default_name;
  const char *filter_name;
  const char *filter_exts;
} FlexFileDialog;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * 库版本（主*10000 + 次*100 + 补丁）。
 */
uint32_t flex_version(void);

/**
 * 校验并构建一段 XML，返回根节点的顶层子节点数量；出错返回负数。
 *
 * 这是一个「非阻塞」入口，便于在无 GUI 环境验证 C ABI 边界。
 * -1: 空指针/编码错误；-2: 解析/构建失败；-3: 内部 panic。
 */
int flex_load_check(const char *xml);

/**
 * 注册全局点击回调：任意「有 name 的按钮」被点击时回调 `cb(name, user)`。
 */
void flex_set_click_callback(void (*cb)(const char *name, void *user), void *user);

/**
 * 设置某具名控件的文本。
 */
void flex_ctx_set_text(void *ctx, const char *name, const char *text);

/**
 * 读取某具名控件的文本到 out 缓冲；返回长度，未找到/出错返回 -1。
 */
int flex_ctx_get_text(void *ctx, const char *name, char *out, int out_len);

/**
 * 读取某具名控件的 selected（CheckBox/Radio）：1=选中，0=未选，-1=未找到。
 */
int flex_ctx_is_selected(void *ctx, const char *name);

/**
 * 设置某具名控件是否可用。
 */
void flex_ctx_set_enabled(void *ctx, const char *name, int enabled);

/**
 * 设置窗口标题。
 */
void flex_ctx_set_title(void *ctx, const char *title);

/**
 * 请求关闭窗口。
 */
void flex_ctx_close(void *ctx);

/**
 * 设置进度条 / 滑块等控件的数值。
 */
void flex_ctx_set_value(void *ctx, const char *name, float value);

/**
 * 读控件数值到 *out；返回 1=已取到，0=未找到/出错。
 */
int flex_ctx_get_value(void *ctx, const char *name, float *out);

/**
 * 设置 CheckBox / Radio 等的选中态。
 */
void flex_ctx_set_selected(void *ctx, const char *name, int selected);

/**
 * 设置下拉 / 分段等的选中项索引（负数忽略）。
 */
void flex_ctx_set_selected_index(void *ctx, const char *name, int index);

/**
 * 读选中项索引到 *out；返回 1=已取到，0=未找到。
 */
int flex_ctx_get_selected_index(void *ctx, const char *name, int *out);

/**
 * 设置控件可见性。
 */
void flex_ctx_set_visible(void *ctx, const char *name, int visible);

/**
 * 读控件可见性：1=可见，0=隐藏，-1=未找到。
 */
int flex_ctx_is_visible(void *ctx, const char *name);

/**
 * 读控件可用性：1=可用，0=禁用，-1=未找到。
 */
int flex_ctx_is_enabled(void *ctx, const char *name);

/**
 * 设置输入框占位提示文本。
 */
void flex_ctx_set_placeholder(void *ctx, const char *name, const char *text);

/**
 * 通知数据源控件（列表 / 虚拟列表）刷新数据；返回 1=已处理，0=未找到。
 */
int flex_ctx_refresh_data(void *ctx,
                          const char *name);

/**
 * 解析一段 XML 片段并追加为具名容器的子节点；返回 1=成功，0=未找到容器/解析失败。
 */
int flex_ctx_add_child_xml(void *ctx,
                           const char *name,
                           const char *xml);

/**
 * 清空具名容器的所有子节点；返回 1=已处理，0=未找到。
 */
int flex_ctx_clear_children(void *ctx, const char *name);

/**
 * 显示窗口。
 */
void flex_ctx_show(void *ctx);

/**
 * 隐藏窗口。
 */
void flex_ctx_hide(void *ctx);

/**
 * 最小化窗口。
 */
void flex_ctx_minimize(void *ctx);

/**
 * 最大化窗口。
 */
void flex_ctx_maximize(void *ctx);

/**
 * 还原窗口（取消最小/最大化）。
 */
void flex_ctx_restore(void *ctx);

/**
 * 退出整个应用（结束事件循环）。
 */
void flex_ctx_quit(void *ctx);

/**
 * 请求整窗重绘。
 */
void flex_ctx_request_redraw(void *ctx);

/**
 * 请求重新布局。
 */
void flex_ctx_request_layout(void *ctx);

/**
 * 切换界面语言（BCP-47，如 "zh-CN"/"en"）；返回 1=成功，0=失败。
 */
int flex_ctx_set_locale(void *ctx, const char *locale);

/**
 * 把具名控件的文本绑定到某本地化资源键（随语言切换自动更新）。
 */
void flex_ctx_set_localized_text(void *ctx, const char *name, const char *resource_key);

/**
 * 从窗口回调取得 UI 线程投递句柄；调用方负责用 `flex_main_proxy_free` 释放。
 */
FlexMainProxy *flex_ctx_main_proxy(void *ctx);

/**
 * 克隆投递句柄，便于多个工作线程分别持有；返回值需要单独释放。
 */
FlexMainProxy *flex_main_proxy_clone(const FlexMainProxy *proxy);

/**
 * 投递 C 回调到所属窗口的 UI 线程；1 表示已接受，0 表示窗口已关闭或参数无效。
 */
int flex_main_proxy_post(const FlexMainProxy *proxy,
                         void (*task)(void *ctx, void *user),
                         void *user);

/**
 * 释放一个投递句柄。传 NULL 无操作。
 */
void flex_main_proxy_free(FlexMainProxy *proxy);

/**
 * 用 XML + C 委托启动应用（阻塞）。delegate 为 NULL 时等价于 flex_run_xml。
 * 0 成功，负数错误码（-1 参数错、-2 XML 失败、-3 panic、-100 无后端）。
 */
int flex_run(const char *title,
             int width,
             int height,
             const char *xml,
             const FlexDelegate *delegate,
             void *user);

/**
 * 读系统剪贴板文本到 out（含 NUL）；返回长度，空/出错返回 -1。
 */
int flex_clipboard_get_text(char *out, int out_len);

/**
 * 写系统剪贴板文本。
 */
void flex_clipboard_set_text(const char *text);

/**
 * 打开文件对话框，路径写入 out；返回长度，取消/出错 -1。
 */
int flex_dialog_open_file(const FlexFileDialog *opts, char *out, int out_len);

/**
 * 打开目录对话框。
 */
int flex_dialog_open_directory(const FlexFileDialog *opts, char *out, int out_len);

/**
 * 保存文件对话框。
 */
int flex_dialog_save_file(const FlexFileDialog *opts, char *out, int out_len);

/**
 * 保存到目录对话框。
 */
int flex_dialog_save_directory(const FlexFileDialog *opts, char *out, int out_len);

/**
 * 用 XML 描述启动应用并进入主事件循环（阻塞）。0 成功，负数见错误码
 * （-1 参数错、-2 XML 失败、-3 panic、-100 该平台无后端）。
 *
 * 会为所有「有 name 的按钮」自动挂接点击回调（转发到 flex_set_click_callback 注册的函数）。
 * 内部按平台 cfg 二选一，保证 C 头文件里只有一份声明。
 */
int flex_run_xml(const char *title,
                 int width,
                 int height,
                 const char *xml);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* FLEXUI_H */
