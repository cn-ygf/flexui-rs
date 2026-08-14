# flexui-rs · XML 布局与控件属性参考

本文档详细说明 flexui-rs 的 XML 布局语法、所有控件及其属性、样式系统、条件渲染与事件响应。所有属性名、取值格式均与实现保持一致。

---

## 目录

1. [快速开始](#1-快速开始)
2. [加载 XML](#2-加载-xml)
3. [`<Window>` 根节点](#3-window-根节点)
4. [通用属性（所有控件可用）](#4-通用属性所有控件可用)
5. [取值格式](#5-取值格式)
6. [分状态样式属性](#6-分状态样式属性)
7. [控件清单与专属属性](#7-控件清单与专属属性)
8. [条件渲染 `v-if` 与平台谓词](#8-条件渲染-v-if-与平台谓词)
9. [`<Include>` 子 XML 复用](#9-include-子-xml-复用)
10. [事件响应](#10-事件响应)
11. [完整示例](#11-完整示例)

---

## 1. 快速开始

一段最小 XML：

```xml
<VBox spacing="10" padding="16" normal-bgcolor="#1A1C22">
  <Label text="你好，flexui" normal-fgcolor="#FFFFFF" bold="true" font-size="16"/>
  <Button name="ok" text="确定" width="120" height="40"
          normal-bgcolor="#3478F6" hot-bgcolor="#4A8CFF" normal-fgcolor="#FFFFFF" corner-radius="8"/>
</VBox>
```

- 标签名**不区分大小写**（`<VBox>`、`<vbox>` 等价）。
- 属性名**不区分大小写**。
- 根节点可以是任意容器；也可以是 `<Window>`（见第 3 节）。

---

## 2. 加载 XML

| 场景 | API | 说明 |
| --- | --- | --- |
| 直接给 XML 字符串 | `Skin::xml("<VBox>…")` | 图片按**文件路径**解析 |
| 走资源系统（目录/zip/内嵌） | `Skin::res("main.xml")` | XML 与图片都经 `ResourceManager` 逻辑路径解析，支持 `<Include>` |
| 代码构建控件树 | `Skin::tree(node)` | 不经 XML |

在 `WindowImpl::skin()` 里返回上述之一。若 XML 根是 `<Window>`，其属性会覆盖 `config()` 提供窗口配置。

---

## 3. `<Window>` 根节点

当 XML 根标签是 `<Window>` 时，它同时描述**窗口配置**与**界面内容**（子节点即内容；多个子节点会自动包进一个 `VBox`）。

| 属性 | 取值 | 默认 | 说明 |
| --- | --- | --- | --- |
| `title` | 字符串 | `flexui-rs` | 窗口标题 |
| `width` | 数字 | `640` | 逻辑像素宽 |
| `height` | 数字 | `440` | 逻辑像素高 |
| `resizable` | 布尔 | `true` | 是否允许改变大小 |
| `titlebar` | `system` / `hidden`(=`hiddenkeepcontrols`) / `none`(=`borderless`) | `system` | 标题栏模式：系统栏 / 隐藏标题栏保留窗口控制（macOS 保留交通灯）/ 无边框自绘 |

```xml
<Window title="演示" width="800" height="560" titlebar="hidden" resizable="false">
  <VBox padding="16"> … </VBox>
</Window>
```

---

## 4. 通用属性（所有控件可用）

以下属性可用于任意控件标签。

### 4.1 标识与文本

| 属性 | 取值 | 说明 |
| --- | --- | --- |
| `name` | 字符串 | 控件名，供事件回调与 `ctx.with(name, …)` 查找 |
| `text` | 字符串 | 文本内容（Label/Button/Edit/CheckBox/Radio 等） |
| `tooltip` | 字符串 | 悬停约 0.5s 后显示的提示气泡 |

### 4.2 字体

| 属性 | 取值 | 说明 |
| --- | --- | --- |
| `font-size` / `fontsize` | 数字 | 字号（逻辑像素） |
| `font-family` / `font` | 字符串 | 字族名 |
| `bold` | 布尔 | 粗体 |
| `italic` | 布尔 | 斜体 |
| `underline` | 布尔 | 下划线 |

### 4.3 尺寸与盒模型

| 属性 | 取值 | 说明 |
| --- | --- | --- |
| `width` | [尺寸值](#51-尺寸值) | 宽度：固定数值 / `content` / `fill` |
| `height` | [尺寸值](#51-尺寸值) | 高度 |
| `padding` | [边距简写](#53-边距简写) | 内边距（内容区四周留白） |
| `margin` | [边距简写](#53-边距简写) | 外边距（参与容器排布） |
| `flex` | 数字 | 主轴伸缩权重（在 VBox/HBox 中分配剩余空间；0 表示不伸缩） |

### 4.4 布局与定位

| 属性 | 取值 | 适用 | 说明 |
| --- | --- | --- | --- |
| `spacing` | 数字 | VBox/HBox/ListView | 子控件间距 |
| `justify` | `start` / `center` / `end` / `space-between`(=`between`) / `space-around`(=`around`) | VBox/HBox | 主轴对齐 |
| `align` | `stretch` / `start` / `center` / `end` | VBox/HBox | 交叉轴对齐 |
| `x` / `y` | 数字 | Box 容器内子控件 | 绝对定位（相对父内容区左上角） |

### 4.5 交互与状态

| 属性 | 取值 | 说明 |
| --- | --- | --- |
| `enabled` | 布尔 | 是否可用（false = disabled 状态） |
| `mouse` | `solid`(默认) / `transparent` | 命中策略：`transparent` 时事件穿透到下层 |
| `multiline` | 布尔 | Edit 多行模式（Enter 换行） |
| `value` | 0~1 | Progress/Slider 的归一化数值 |

### 4.6 分组与选择

| 属性 | 取值 | 适用 | 说明 |
| --- | --- | --- | --- |
| `group` | 整数 | Radio | 单选分组号（同组互斥） |
| `tab-index` / `tabindex` | 整数 | Radio | 关联 TabBox 的页序号 |
| `checked` | 布尔 | CheckBox/Radio | 初始选中 |
| `selected` | 布尔 **或** 整数 | 见说明 | 对 `tabbox`/`combobox`/`listview` 为**页/项序号**（整数）；其它控件为**选中布尔** |

---

## 5. 取值格式

### 5.1 尺寸值

`width`/`height` 接受：

| 写法 | 含义 |
| --- | --- |
| 数字（如 `120`） | 固定尺寸（逻辑像素） |
| `content` / `auto` | 按内容自适应 |
| `fill` / `stretch` | 撑满可用空间（等价于 `flex`） |

### 5.2 布尔值

`true` / `1` / `yes` / `on` 视为真，其余为假。

### 5.3 边距简写

`padding`/`margin` 支持 CSS 风格 1/2/3/4 个数值（空格或逗号分隔）：

| 写法 | 含义 |
| --- | --- |
| `10` | 四边都是 10 |
| `10 20` | 上下=10，左右=20 |
| `10 20 30` | 上=10，左右=20，下=30 |
| `10 20 30 40` | 上=10，右=20，下=30，左=40（CSS 顺序 上 右 下 左） |

### 5.4 颜色

`#RGB` / `#RRGGBB` / `#AARRGGBB`（**8 位时 alpha 在最前**）：

- `#F00` = 不透明红
- `#3478F6` = 蓝
- `#80000000` = 50% 透明黑

---

## 6. 分状态样式属性

样式属性可带**状态前缀**，实现 4×2（+selected）状态机分状态换肤。前缀由「基础状态 + focus + selected」组成，顺序任意，用连字符连接，最后接属性名。无前缀等价于 `normal`。

**基础状态**：`normal` / `hot`（悬停）/ `pushed`（按下）/ `disabled`（禁用）
**附加维度**：`focus`（获得焦点）/ `selected`（勾选/单选选中）

未命中的状态会按「具体→一般」回退，字段级最终回退到 `normal`。

示例：

```xml
<Button
  normal-bgcolor="#3478F6"
  hot-bgcolor="#4A8CFF"
  pushed-bgcolor="#2A5FD0"
  disabled-bgcolor="#5A5F6B"
  focus-bordercolor="#FFFFFF"
  normal-fgcolor="#FFFFFF"
  corner-radius="8"/>
```

### 样式属性清单

| 属性名（可加状态前缀） | 取值 | 说明 |
| --- | --- | --- |
| `bgcolor` | 颜色 | 背景色 |
| `fgcolor` | 颜色 | 前景色（文字/图标） |
| `bordercolor` | 颜色 | 边框色 |
| `border-width` | 数字 | 边框宽度 |
| `corner-radius` | 数字 | 圆角半径（四角相同） |
| `textalign` | `left` / `center` / `right` | 文本对齐 |
| `opacity` | 0~1 | 控件自身透明度（作用于本控件的背景/内容/边框，不含子控件） |
| `bggradient` / `gradient` | `色A,色B[,h\|v]` | 两色线性渐变背景（默认竖直，`h`/`horizontal` 为水平）；存在时优先于 `bgcolor` |
| `shadow` | `dx dy #色` | 硬阴影（按偏移画同形填充，如 `0 3 #66000000`） |
| `bgimage` | 图片路径 | 背景图 |
| `bgtint` | 颜色 | 背景图换色（黑图动态着色，保留 alpha 形状） |
| `bgfit` | [渲染方式](#61-图片渲染方式) | 背景图渲染方式 |
| `fgimage` | 图片路径 | 前景图 |
| `fgtint` | 颜色 | 前景图换色 |
| `fgfit` | [渲染方式](#61-图片渲染方式) | 前景图渲染方式 |

> 注：`border-width` / `corner-radius` 属性名内部忽略连字符，`normal-border-width` 与 `border-width`（=normal）均可。

### 6.1 图片渲染方式

`bgfit`/`fgfit` 取值：

| 写法 | 含义 |
| --- | --- |
| `stretch` | 拉伸填满（默认） |
| `center` | 原尺寸居中 |
| `tile` | 平铺 |
| `ninepatch` 或 `ninepatch(l,t,r,b)` | 九宫格：四边不拉伸、中间拉伸；括号内为源图四边保护边距 |

---

## 7. 控件清单与专属属性

标签名不区分大小写；下表「标签」列给出可用别名。除专属属性外，均可使用[通用属性](#4-通用属性所有控件可用)与[分状态样式](#6-分状态样式属性)。

### 容器

| 标签 | 说明 | 专属/常用属性 |
| --- | --- | --- |
| `VBox` | 纵向排列容器 | `spacing`、`justify`、`align` |
| `HBox` | 横向排列容器 | `spacing`、`justify`、`align` |
| `Box` / `Panel` | 叠放容器（子控件各自填充内容区；可配合 `x`/`y` 绝对定位） | — |
| `TabBox` | 多页容器（按当前页显示某个子节点） | `bindgroup`（绑定的 Radio 分组号）、`selected`（初始页序号） |
| `Scroll` / `ScrollView` | 纵向滚动容器 | `spacing` |

### 基础控件

| 标签 | 说明 | 专属/常用属性 |
| --- | --- | --- |
| `Label` | 文本标签 | `text`；对齐用 `textalign` 样式（超宽自动省略号截断） |
| `Button` | 按钮（完整 4 状态） | `text`；点击经 `name` 上报 |
| `CheckBox` | 勾选框 | `text`、`checked` |
| `Radio` | 单选 | `text`、`group`、`tab-index`、`checked`/`selected` |
| `Edit` | 文本输入 | `text`、`multiline`；支持选区/剪贴板/IME（见运行时） |
| `Image` | 图片 | `src`（来源路径，`.svg` 自动矢量光栅化）；换色/渲染用 `fgtint`/`fgfit` 样式 |

### 扩展控件

| 标签 | 说明 | 专属属性 |
| --- | --- | --- |
| `Progress` | 进度条 | `value`（0~1） |
| `Slider` | 滑块 | `value`（0~1，拖动改变） |
| `ComboBox` / `Select` | 下拉选择框 | `options="a,b,c"` 或 `<item text="…"/>` 子元素；`selected`（初始项序号） |
| `ListView` / `List` | 可滚动列表（点击选中） | `items="a,b,c"` 或 `<item text="…"/>` 子元素；`selected`（初始行）、`row-height`（行高） |
| `Separator` / `Hr` | 分隔条 | `orientation`（`horizontal`默认/`vertical`）、`thickness`（线粗） |

> `<item>` 子元素用 `text` 或 `label` 属性给出文本；它们只作为数据，不会成为控件子节点。

### TabBox + Radio 联动（做 tabbar）

`Radio` 用 `group` 分组、`tab-index` 指定页序号；`TabBox` 用 `bindgroup` 绑定同一分组号。点击某个 Radio 即切换 TabBox 到对应页：

```xml
<HBox spacing="8">
  <Radio group="1" tab-index="0" selected="true" text="页一"/>
  <Radio group="1" tab-index="1" text="页二"/>
  <Radio group="1" tab-index="2" text="页三"/>
</HBox>
<TabBox bindgroup="1" flex="1">
  <Box> 第一页 </Box>
  <Box> 第二页 </Box>
  <Box> 第三页 </Box>
</TabBox>
```

---

## 8. 条件渲染 `v-if` 与平台谓词

任意控件可加 `v-if="<表达式>"`，在**加载期**静态求值；为假则整棵子树不生成。

**表达式语法**：布尔标识符 + 运算符 `&&`、`||`、`!`、圆括号 `()`。

**内置平台谓词**（按编译目标自动注入）：

- `platform.macos`
- `platform.windows`

**自定义变量**：通过代码 `Context::new().set("已登录", true)` 注入，再在 XML 用 `v-if="已登录 && !平台.windows"` 等。

```xml
<!-- 仅在非 macOS 上渲染这组系统按钮 -->
<HBox v-if="!platform.macos">
  <Button text="最小化"/><Button text="关闭"/>
</HBox>
```

---

## 9. `<Include>` 子 XML 复用

把另一份 XML 就地展开（需经资源系统加载，即 `Skin::res` / `load_window_res`）：

```xml
<VBox>
  <Include src="toolbar.xml"/>
  <Include src="content.xml"/>
</VBox>
```

- `src` 为资源逻辑路径。
- 带**循环引用检测**与**嵌套深度限制**。

---

## 10. 事件响应

XML 只描述界面；事件在代码里按 `name` 处理（实现 `WindowImpl`）：

| 钩子 | 触发 |
| --- | --- |
| `on_before_init(ctx)` / `on_init(ctx)` / `on_initialized(ctx)` | 控件树建立后依次执行的初始化前、初始化、初始化完成事件 |
| `on_click(name, ctx)` | 具名 Button/CheckBox/Radio 点击、ComboBox 选中（上报下拉框 name）、ListView 选中行（上报列表 name）、右键菜单项选中 |
| `on_context(name, x, y, ctx)` | 右键具名控件（可在此 `ctx.open_menu(...)` 弹上下文菜单） |
| `on_window_event(event, ctx)` | 窗口移动、缩放、焦点、最小化、最大化、恢复及键盘事件 |
| `on_key(key, ctx)` / `on_size(w,h,ctx)` | 兼容用的键盘和尺寸事件 |
| `on_closing(ctx)` / `on_closed()` | 即将关闭（返回 `false` 可取消）和原生窗口关闭后；旧 `on_close(ctx)` 仍兼容 |
| `on_message(msg, ctx)` | 后台线程经 `MainProxy` 投递的消息 |

`ctx`（`WindowCtx`）常用能力：

```rust
ctx.set_text("status", "已保存");           // 改某控件文本
ctx.is_selected("chkRemember");             // 读勾选态
ctx.set_enabled("btnPrimary", false);       // 启用/禁用
ctx.with("cb", |w| w.base().text.clone());  // 任意读写控件
ctx.animate("prog", AnimProp::Value, 1.0, 0.8, Easing::EaseInOut); // 属性动画
ctx.open_menu(Rect::new(x, y, 0.0, 0.0), vec![("刷新".into(), "ctxRefresh".into())]); // 上下文菜单
```

工作线程或任意 async runtime 完成任务后，通过初始化阶段取得的 `MainProxy` 投递回 UI
线程。闭包中的 setter 会自动触发布局或重绘：

```rust
let ui = ctx.main_proxy().expect("window proxy");
std::thread::spawn(move || {
    let result = load_data();
    ui.post(move |ctx| ctx.set_text("status", result));
});
```

---

## 11. 完整示例

```xml
<Window title="控件总览" width="680" height="560" titlebar="system">
  <VBox spacing="12" padding="16" normal-bgcolor="#1A1C22">

    <Label text="flexui-rs 控件总览" height="26" normal-fgcolor="#FFFFFF" bold="true" font-size="16"/>
    <Label name="status" text="状态：就绪" height="24" normal-fgcolor="#8FE3A0"/>

    <!-- 按钮：渐变 + 阴影 + tooltip -->
    <HBox spacing="10" height="44">
      <Button name="btnPrimary" text="主要按钮" width="120" height="40" tooltip="主操作"
              normal-bggradient="#4A8CFF,#2A5FD0" normal-shadow="0 3 #66000000"
              hot-bgcolor="#5A9CFF" normal-fgcolor="#FFFFFF" corner-radius="8"/>
      <Button name="btnOpen" text="选择文件…" width="110" height="40"
              normal-bgcolor="#2A6E4F" normal-fgcolor="#FFFFFF" corner-radius="8"/>
    </HBox>

    <!-- 下拉框 -->
    <HBox spacing="12" height="34">
      <Label text="主题：" width="50" height="26" normal-fgcolor="#E6EBF5"/>
      <ComboBox name="theme" options="深色,浅色,跟随系统" selected="0" width="140" height="30"
                normal-bgcolor="#20242C" normal-fgcolor="#E6EBF5"
                border-width="1" normal-bordercolor="#4A5064" corner-radius="6"/>
    </HBox>

    <!-- 复选框 + 单选 tabbar -->
    <HBox spacing="24" height="28">
      <CheckBox name="chkRemember" text="记住我" normal-fgcolor="#E6EBF5"/>
      <CheckBox name="chkNews" text="订阅通知" checked="true" normal-fgcolor="#E6EBF5"/>
    </HBox>
    <HBox spacing="8" height="26">
      <Radio group="1" tab-index="0" selected="true" text="页一" width="90" normal-fgcolor="#E6EBF5"/>
      <Radio group="1" tab-index="1" text="页二" width="90" normal-fgcolor="#E6EBF5"/>
    </HBox>

    <!-- 多页容器 -->
    <TabBox bindgroup="1" flex="1">
      <VBox normal-bgcolor="#2C3E5A" corner-radius="10" padding="14" spacing="10">
        <Edit name="edit1" text="在这里输入…" height="32"
              normal-bgcolor="#20242C" normal-fgcolor="#FFFFFF"
              border-width="1" normal-bordercolor="#4A5064" corner-radius="6"/>
        <Separator normal-bgcolor="#4A5064"/>
        <Slider name="vol" value="0.4" height="24" normal-bgcolor="#20242C" normal-fgcolor="#3478F6"/>
        <Progress name="prog" value="0.65" height="8" normal-bgcolor="#20242C" normal-fgcolor="#8FE3A0"/>
      </VBox>
      <VBox normal-bgcolor="#2C4A3E" corner-radius="10" padding="12">
        <ListView name="cities" flex="1" row-height="26"
                  normal-bgcolor="#20242C" border-width="1" normal-bordercolor="#4A5064"
                  items="北京,上海,广州,深圳,杭州,成都"/>
      </VBox>
    </TabBox>

  </VBox>
</Window>
```

对应事件（节选）：

```rust
fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
    match name {
        "btnPrimary" => ctx.animate("prog", AnimProp::Value, 1.0, 0.8, Easing::EaseInOut),
        "btnOpen" => {
            let opts = flexui::dialog::FileDialog::new().title("选择图片").filter("图片", &["png","jpg"]);
            if let Some(p) = flexui::dialog::open_file(&opts) {
                ctx.set_text("status", format!("选择了 {}", p.display()));
            }
        }
        "theme" => {
            let cur = ctx.with("theme", |w| w.base().text.clone()).unwrap_or_default();
            ctx.set_text("status", format!("主题切换为 {cur}"));
        }
        _ => {}
    }
}
```
