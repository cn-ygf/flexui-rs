//! Linux XIM 客户端：接系统输入法（ibus/fcitx 经 XIM 桥），支持中文等合成输入。
//!
//! 纯 Rust，复用后端已有的 x11rb 连接，不链接 libX11。设计上把 `ImeSink` 当成被动
//! 「邮箱」：`filter_event` 触发的回调只把结果堆进去——提交串(commit，输入法确定的最终
//! 文字)、预编辑串(preedit，正在合成、带下划线的临时文字)、转发键(forward，输入法没吃掉、
//! 原样退回的按键：普通英文/方向键/快捷键等)。
//! 事件循环随后 `drain()` 取走并应用到聚焦控件（提交→逐字符 Char、预编辑→marked text、
//! 转发键→照常走按键处理）。与 macOS 的 NSTextInputClient 一致：core 已有
//! `set_marked_text` / `clear_marked_text` / `Event::Char`。
//!
//! 无输入法服务器时 `Ime::new` 返回 `None`，后端回落到直接 keysym→字符 的路径。

use x11rb::protocol::xproto::{KeyPressEvent, Window};
use x11rb::protocol::Event as XEvent;
use x11rb::rust_connection::RustConnection;

use xim::x11rb::{HasConnection, X11rbClient};
use xim::{
    AHashMap, AttributeName, Client, ClientCore, ClientError, ClientHandler, Feedback,
    ForwardEventFlag, InputStyle, Point, PreeditDrawStatus,
};

/// 把后端主连接（借用）交给 xim 用，避免 xim 独占一条连接。
struct ConnRef<'a>(&'a RustConnection);

impl HasConnection for ConnRef<'_> {
    type Connection = RustConnection;
    fn conn(&self) -> &RustConnection {
        self.0
    }
}

/// 回调产出的、待事件循环应用的一批输入法结果。
#[derive(Default)]
struct ImeSink {
    im_id: u16,
    ic_id: u16,
    /// IC 建好、可转发按键。
    ready: bool,
    /// IC 绑定的窗口（提交/预编辑落到此窗口的聚焦控件）。
    window: Window,
    /// 提交串（可能一次多段）。
    commits: Vec<String>,
    /// 输入法未消费、原样退回的按键：(keycode, state 位, 是否按下)。
    keys: Vec<(u8, u16, bool)>,
    /// 预编辑串更新；`Some("")` 表示清除。None 表示本批无变化。
    preedit: Option<String>,
}

impl<T, C> ClientHandler<C> for ImeSink
where
    C: Client<XEvent = T> + ClientCore<XEvent = T>,
{
    fn handle_connect(&mut self, client: &mut C) -> Result<(), ClientError> {
        // 打开输入法：locale 交给输入法服务器自行决定（ibus/fcitx 按当前引擎合成）。
        client.open("")
    }

    fn handle_open(&mut self, client: &mut C, input_method_id: u16) -> Result<(), ClientError> {
        self.im_id = input_method_id;
        client.get_im_values(input_method_id, &[AttributeName::QueryInputStyle])
    }

    fn handle_get_im_values(
        &mut self,
        client: &mut C,
        input_method_id: u16,
        _attributes: AHashMap<AttributeName, Vec<u8>>,
    ) -> Result<(), ClientError> {
        // on-the-spot 预编辑：合成串经 PREEDIT_CALLBACKS 回调交我们内嵌绘制。
        let ic_attributes = client
            .build_ic_attributes()
            .push(
                AttributeName::InputStyle,
                InputStyle::PREEDIT_CALLBACKS | InputStyle::STATUS_NOTHING,
            )
            .push(AttributeName::ClientWindow, self.window)
            .push(AttributeName::FocusWindow, self.window)
            .nested_list(AttributeName::PreeditAttributes, |b| {
                b.push(AttributeName::SpotLocation, Point { x: 0, y: 0 });
            })
            .build();
        client.create_ic(input_method_id, ic_attributes)
    }

    fn handle_create_ic(
        &mut self,
        client: &mut C,
        _input_method_id: u16,
        input_context_id: u16,
    ) -> Result<(), ClientError> {
        self.ic_id = input_context_id;
        self.ready = true;
        // 让此 IC 取得输入焦点，后续按键才会被合成。
        client.set_focus(self.im_id, self.ic_id)
    }

    fn handle_commit(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        text: &str,
    ) -> Result<(), ClientError> {
        self.commits.push(text.to_string());
        Ok(())
    }

    fn handle_forward_event(
        &mut self,
        client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        _flag: ForwardEventFlag,
        xev: T,
    ) -> Result<(), ClientError> {
        // 输入法没吃掉的按键：序列化取 keycode/state/press，退回给正常按键处理。
        let x = client.serialize_event(&xev);
        let is_press = (x.response_type & 0x7f) == 2; // 2=KeyPress, 3=KeyRelease
        self.keys.push((x.detail, x.state, is_press));
        Ok(())
    }

    fn handle_preedit_draw(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
        _caret: i32,
        _chg_first: i32,
        _chg_len: i32,
        _status: PreeditDrawStatus,
        preedit_string: &str,
        _feedbacks: Vec<Feedback>,
    ) -> Result<(), ClientError> {
        self.preedit = Some(preedit_string.to_string());
        Ok(())
    }

    fn handle_preedit_done(
        &mut self,
        _client: &mut C,
        _input_method_id: u16,
        _input_context_id: u16,
    ) -> Result<(), ClientError> {
        self.preedit = Some(String::new());
        Ok(())
    }
}

/// 一批待应用的输入法输出（由事件循环消费）。
pub struct ImeOutput {
    /// IC 绑定的目标窗口。
    pub window: Window,
    /// 提交的最终文字（逐字符发 Char）。
    pub commits: Vec<String>,
    /// 未被消费、退回的按键：(keycode, state 位, 是否按下)。
    pub keys: Vec<(u8, u16, bool)>,
    /// 预编辑串变化；`Some("")` 清除，`None` 无变化。
    pub preedit: Option<String>,
}

impl ImeOutput {
    /// 本批是否什么都没有（可跳过应用）。
    pub fn is_empty(&self) -> bool {
        self.commits.is_empty() && self.keys.is_empty() && self.preedit.is_none()
    }
}

/// XIM 客户端封装：借用后端主连接，绑定到一个目标窗口做合成输入。
pub struct Ime<'a> {
    client: X11rbClient<ConnRef<'a>>,
    sink: ImeSink,
}

impl<'a> Ime<'a> {
    /// 尝试连接 XIM。无输入法服务器/初始化失败则返回 None（后端回落直接按键）。
    pub fn new(conn: &'a RustConnection, screen_num: usize, window: Window) -> Option<Self> {
        let client = X11rbClient::init(ConnRef(conn), screen_num, None).ok()?;
        let sink = ImeSink {
            window,
            ..ImeSink::default()
        };
        Some(Ime { client, sink })
    }

    /// 处理一个 X 事件：若是 XIM 协议事件则消费并触发回调，返回是否已消费。
    pub fn filter(&mut self, ev: &XEvent) -> bool {
        self.client
            .filter_event(ev, &mut self.sink)
            .unwrap_or(false)
    }

    /// IC 是否就绪（可转发按键）。
    pub fn ready(&self) -> bool {
        self.sink.ready
    }

    /// 把一个按键事件转发给输入法（KeyPress/KeyRelease 共用 KeyPressEvent）。
    pub fn forward_key(&mut self, ev: &KeyPressEvent) {
        if self.sink.ready {
            let _ = self.client.forward_event(
                self.sink.im_id,
                self.sink.ic_id,
                ForwardEventFlag::empty(),
                ev,
            );
        }
    }

    /// 取走本批输出并清空邮箱。
    pub fn drain(&mut self) -> ImeOutput {
        ImeOutput {
            window: self.sink.window,
            commits: std::mem::take(&mut self.sink.commits),
            keys: std::mem::take(&mut self.sink.keys),
            preedit: self.sink.preedit.take(),
        }
    }
}
