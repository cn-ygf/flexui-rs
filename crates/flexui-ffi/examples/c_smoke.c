/* flexui-rs C ABI 冒烟测试：验证从 C 调用 flexui 动态库（非阻塞路径）。
 * 编译运行见 crates/flexui-ffi/examples/run_c_smoke.sh */
#include <stdio.h>
#include "flexui.h"

int main(void) {
    unsigned int v = flex_version();
    printf("flex_version = %u\n", v);

    const char *xml =
        "<VBox spacing=\"10\">"
        "  <Button name=\"ok\" text=\"OK\"/>"
        "  <Label text=\"hi\"/>"
        "  <HBox v-if=\"!platform.macos\"><Button text=\"min\"/></HBox>"
        "</VBox>";
    int n = flex_load_check(xml);
    printf("flex_load_check = %d\n", n);

    /* 链接性检查：引用新导出的符号地址（不调用阻塞入口）。 */
    void *syms[] = {
        (void *)flex_run,
        (void *)flex_ctx_set_text,
        (void *)flex_ctx_get_text,
        (void *)flex_ctx_is_selected,
        (void *)flex_ctx_set_enabled,
        (void *)flex_ctx_set_title,
        (void *)flex_ctx_close,
        (void *)flex_dialog_open_file,
        (void *)flex_dialog_open_directory,
        (void *)flex_dialog_save_file,
        (void *)flex_dialog_save_directory,
        (void *)flex_clipboard_get_text,
        (void *)flex_clipboard_set_text,
    };
    int nsyms = (int)(sizeof(syms) / sizeof(syms[0]));
    printf("linked %d extra symbols\n", nsyms);

    /* macOS 上 v-if=!platform.macos 为假 → HBox 被裁剪，顶层应为 2 个子节点。 */
    if (v == 1 && n == 2 && nsyms == 13) {
        printf("C-ABI-OK\n");
        return 0;
    }
    printf("C-ABI-FAIL\n");
    return 1;
}
