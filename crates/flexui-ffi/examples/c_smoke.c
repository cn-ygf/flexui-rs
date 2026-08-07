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

    /* macOS 上 v-if=!platform.macos 为假 → HBox 被裁剪，顶层应为 2 个子节点。 */
    if (v == 1 && n == 2) {
        printf("C-ABI-OK\n");
        return 0;
    }
    printf("C-ABI-FAIL\n");
    return 1;
}
