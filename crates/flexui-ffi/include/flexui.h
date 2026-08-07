/* flexui-rs C ABI 头文件（手写；后续可用 cbindgen 自动生成）。 */
#ifndef FLEXUI_H
#define FLEXUI_H

#ifdef __cplusplus
extern "C" {
#endif

/* 库版本。 */
unsigned int flex_version(void);

/* 校验并构建一段 XML，返回根节点顶层子节点数；出错返回负数。
 * -1 空指针/编码错误；-2 解析/构建失败；-3 内部 panic。 */
int flex_load_check(const char *xml);

/* 点击回调类型：name 为按钮的 name，user 为注册时传入的用户数据。 */
typedef void (*FlexClickCb)(const char *name, void *user);

/* 注册全局点击回调（传 NULL 取消）。 */
void flex_set_click_callback(FlexClickCb cb, void *user);

/* 用 XML 启动应用并进入主事件循环（阻塞）。0 成功，负数为错误码。 */
int flex_run_xml(const char *title, int width, int height, const char *xml);

#ifdef __cplusplus
}
#endif

#endif /* FLEXUI_H */
