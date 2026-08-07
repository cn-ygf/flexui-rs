# flexui-rs 布局与 XML 描述（细节需求）

对应需求：代码布局（类 QWidget）、XML 布局（类 duilib）、v-if 条件渲染（类 Vue）、
按平台条件渲染（如非 macOS 才画系统标题栏按钮）。

## 1. 两种布局方式（需求 10）

同一套控件树，支持两种构建方式，二者最终生成相同的 `Widget` 树：

### 1.1 代码布局（类 QWidget）
命令式 / Builder 风格，Rust 里直接搭树：

```rust
let root = VBox::new()
    .push(Label::new("标题").fg_color(WHITE))
    .push(
        HBox::new()
            .push(Button::new("确定").on_click(|_| { /* ... */ }))
            .push(Button::new("取消"))
    );
window.set_root(root);
```

### 1.2 XML 布局（类 duilib）
运行时加载 XML 描述界面，解析成控件树。属性名与第 `06` 文的样式/状态字段对应：

```xml
<VBox>
  <Label text="标题" fgcolor="#FFFFFF"/>
  <HBox>
    <Button name="ok" text="确定"
            normal-bgcolor="#3478F6" hot-bgcolor="#4A88FF"
            pushed-bgcolor="#2A5FD0" disabled-bgcolor="#999999"
            border-width="1" border-color="#2A5FD0" corner-radius="6"/>
    <Button name="cancel" text="取消"/>
  </HBox>
</VBox>
```

- **分状态样式的 XML 表达**：属性前缀 `normal-/hot-/pushed-/disabled-`，
  focus 变体再加 `focus-`（如 `hot-focus-bgcolor`）。缺省回退到 `normal-`。
- **容器**：`<Box>/<VBox>/<HBox>` 多子；普通控件标签内嵌一个子标签=单子嵌套。
- **事件穿透**：属性 `mouse="transparent|solid"`。
- **radio/group/tabbox**：`<Radio group="mode" tab-index="0"/>` +
  `<TabBox name="pages">...<page/>...</TabBox>`，用 `group` 与 `tabbox`/`tab-index` 关联成 tabbar。

**解析器**：新增 `flexui-xml` crate，负责「XML → 控件树 + 样式」。一期用简单
DOM 解析（可选轻量 XML 解析绑定，或自写最小解析器）。**属性名 → 控件属性**的映射
用注册表/宏收敛，便于扩展新控件。

> 代码布局与 XML 布局共用同一套 `Widget` 构造入口（Builder 即「属性 setter 集合」），
> XML 解析器只是把属性字符串转成对 Builder 的调用，避免两套逻辑。

## 2. 条件渲染 v-if（需求 10 语法糖）

XML 节点支持 `v-if="表达式"`：表达式为真才把该节点（及子树）纳入控件树。

```xml
<Button v-if="showAdvanced" text="高级设置"/>
<VBox v-if="user.isVip">...</VBox>
```

- **求值上下文**：加载时传入一个「数据上下文」（键值/特性开关/平台信息）。
- **一期实现**：**加载期静态求值**——解析时按上下文决定节点是否生成，条件为假则整棵子树
  不进入控件树。表达式支持基础布尔/比较/取反/与或。
- **响应式（数据变了自动重渲染）**：复杂度高，列为**后续增强**，一期不做。
  需要动态显隐时，一期可用控件 `visible` 属性 + 代码切换。

## 3. 按平台条件渲染（需求 11）

在 `v-if` 的求值上下文里内置**平台谓词**，让布局能按平台裁剪：

- 内置变量：`platform.macos`、`platform.windows`（后续可加 `linux`）。
- 典型用途：**只有非 macOS 才渲染自绘的系统标题栏按钮**（最小化/最大化/还原/关闭）——
  因为 macOS 有原生「红黄绿」交通灯窗口控制，Windows 若用自绘标题栏则需自己画这些按钮。

```xml
<!-- 自绘标题栏：仅在非 macOS 显示系统按钮组 -->
<HBox name="caption-buttons" v-if="!platform.macos">
  <Button name="min"     normal-fgimage="res/min.png"/>
  <Button name="max"     normal-fgimage="res/max.png"/>
  <Button name="restore" normal-fgimage="res/restore.png" v-if="isMaximized"/>
  <Button name="close"   normal-fgimage="res/close.png"/>
</HBox>
```

代码布局侧提供等价能力：`if !Platform::is_macos() { hbox.push(caption_buttons()); }`，
以及查询 API `Platform::current()`，保证两种布局方式一致。

## 4. 布局引擎与本细节的关系

- VBox/HBox 走 Flex 主轴/交叉轴排布；Box 为叠放。
- 每个控件的 `rect` 参数（第 06 文样式字段）可参与布局：显式 rect = 绝对定位覆盖，
  未给 rect = 由父容器 Flex 计算。二者优先级需在布局引擎里定义清楚
  （建议：Box 内优先用显式 rect；V/HBox 内优先走 Flex，rect 仅作尺寸提示）。

## 5. 新增/受影响的工程模块

- 新增 crate：`flexui-xml`（XML 解析 + v-if 求值 + 属性映射）。
- `flexui-core` 增加：状态机、StyleSet、HitPolicy、RadioGroup、TabBox、Platform 谓词。
- 公共 API（L5）同时暴露代码 Builder 与 `Window::load_xml(...)`。
