//! Xdnd 文件拖放目标。
//!
//! 本模块只负责 Xdnd 协议、selection 数据接收和 `text/uri-list` 解析；窗口层负责把
//! [`XdndOutcome::Dropped`] 转交给 `WindowDelegate::on_drop_files`。

use std::time::{Duration, Instant};

use url::Url;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, PropMode,
    PropertyNotifyEvent, SelectionNotifyEvent, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};

/// 本后端声明支持的 Xdnd 协议版本。
const XDND_VERSION: u32 = 5;
/// 单次拖放最多接收 8 MiB，避免异常 selection 无限占用内存。
const MAX_TRANSFER_BYTES: usize = 8 * 1024 * 1024;
/// 从收到 Drop 到完成 selection 传输的最长时间。
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
/// `GetProperty.long_length` 的单位是四字节。
const MAX_PROPERTY_LONGS: u32 = (MAX_TRANSFER_BYTES / 4) as u32;

/// 窗口层转交 Xdnd 事件后得到的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum XdndOutcome {
    /// 不是当前拖放会话的事件，窗口层可继续处理。
    Ignored,
    /// 协议事件已经处理，但尚无文件可交付。
    Handled,
    /// selection 已完整接收并解析为本地绝对路径。
    Dropped(Vec<String>),
}

/// Xdnd 所需的全部 Atom。应用启动时创建一次，供所有窗口共享。
#[derive(Debug, Clone, Copy)]
pub(crate) struct XdndAtoms {
    xdnd_aware: Atom,
    xdnd_enter: Atom,
    xdnd_leave: Atom,
    xdnd_position: Atom,
    xdnd_status: Atom,
    xdnd_selection: Atom,
    xdnd_drop: Atom,
    xdnd_finished: Atom,
    xdnd_type_list: Atom,
    xdnd_action_copy: Atom,
    text_uri_list: Atom,
    xdnd_data: Atom,
    incr: Atom,
}

impl XdndAtoms {
    /// 向 X server 注册/取得 Xdnd Atom。
    pub(crate) fn new(conn: &RustConnection) -> Result<Self, String> {
        Ok(Self {
            xdnd_aware: intern(conn, b"XdndAware")?,
            xdnd_enter: intern(conn, b"XdndEnter")?,
            xdnd_leave: intern(conn, b"XdndLeave")?,
            xdnd_position: intern(conn, b"XdndPosition")?,
            xdnd_status: intern(conn, b"XdndStatus")?,
            xdnd_selection: intern(conn, b"XdndSelection")?,
            xdnd_drop: intern(conn, b"XdndDrop")?,
            xdnd_finished: intern(conn, b"XdndFinished")?,
            xdnd_type_list: intern(conn, b"XdndTypeList")?,
            xdnd_action_copy: intern(conn, b"XdndActionCopy")?,
            text_uri_list: intern(conn, b"text/uri-list")?,
            xdnd_data: intern(conn, b"FLEXUI_XDND_DATA")?,
            incr: intern(conn, b"INCR")?,
        })
    }

    /// 把窗口声明为 Xdnd v5 目标。窗口事件掩码还需包含 `PROPERTY_CHANGE`。
    pub(crate) fn register_window(
        &self,
        conn: &RustConnection,
        window: Window,
    ) -> Result<(), String> {
        conn.change_property32(
            PropMode::REPLACE,
            window,
            self.xdnd_aware,
            AtomEnum::ATOM,
            &[XDND_VERSION],
        )
        .map_err(|error| format!("设置 XdndAware 失败: {error}"))?;
        conn.flush()
            .map_err(|error| format!("刷新 XdndAware 失败: {error}"))
    }
}

/// 单个目标窗口的一次 Xdnd 会话状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Hover,
    AwaitingSelection,
    Incremental,
}

/// 单个窗口持有一份；不持有 X11 连接，可随 `WinState` 一起存放。
#[derive(Debug)]
pub(crate) struct XdndTarget {
    source: Window,
    version: u32,
    target_type: Atom,
    phase: Phase,
    deadline: Option<Instant>,
    buffer: Vec<u8>,
}

impl Default for XdndTarget {
    fn default() -> Self {
        Self {
            source: NONE,
            version: 0,
            target_type: NONE,
            phase: Phase::Idle,
            deadline: None,
            buffer: Vec::new(),
        }
    }
}

impl XdndTarget {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// 处理 `XdndEnter/Leave/Position/Drop` ClientMessage。
    pub(crate) fn handle_client_message(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        event: &ClientMessageEvent,
    ) -> XdndOutcome {
        if event.format != 32 || event.window != target_window {
            return XdndOutcome::Ignored;
        }
        let data = event.data.as_data32();
        if event.type_ == atoms.xdnd_enter {
            self.handle_enter(conn, atoms, target_window, data);
        } else if event.type_ == atoms.xdnd_leave {
            if data[0] != self.source {
                return XdndOutcome::Ignored;
            }
            // Drop 后 selection 仍可能异步到达，此时 Leave 不能打断传输。
            if self.phase == Phase::Hover {
                self.reset();
            }
        } else if event.type_ == atoms.xdnd_position {
            if data[0] != self.source || self.phase != Phase::Hover {
                return XdndOutcome::Ignored;
            }
            send_status(
                conn,
                atoms,
                target_window,
                self.source,
                self.target_type != NONE,
            );
        } else if event.type_ == atoms.xdnd_drop {
            if data[0] != self.source || self.phase != Phase::Hover {
                return XdndOutcome::Ignored;
            }
            if self.target_type == NONE {
                self.finish(conn, atoms, target_window, false);
            } else {
                let timestamp = if self.version >= 1 {
                    data[2]
                } else {
                    CURRENT_TIME
                };
                self.phase = Phase::AwaitingSelection;
                self.deadline = Some(Instant::now() + TRANSFER_TIMEOUT);
                self.buffer.clear();
                if conn
                    .convert_selection(
                        target_window,
                        atoms.xdnd_selection,
                        self.target_type,
                        atoms.xdnd_data,
                        timestamp,
                    )
                    .is_err()
                {
                    self.finish(conn, atoms, target_window, false);
                } else {
                    let _ = conn.flush();
                }
            }
        } else {
            return XdndOutcome::Ignored;
        }
        XdndOutcome::Handled
    }

    /// 处理 `convert_selection` 的结果。普通属性在这里完成；INCR 转入分块阶段。
    pub(crate) fn handle_selection_notify(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        event: &SelectionNotifyEvent,
    ) -> XdndOutcome {
        if self.phase != Phase::AwaitingSelection
            || event.requestor != target_window
            || event.selection != atoms.xdnd_selection
            || event.target != self.target_type
        {
            return XdndOutcome::Ignored;
        }
        if event.property == NONE || event.property != atoms.xdnd_data {
            self.finish(conn, atoms, target_window, false);
            return XdndOutcome::Handled;
        }

        let reply = match conn.get_property(
            false,
            target_window,
            event.property,
            AtomEnum::ANY,
            0,
            MAX_PROPERTY_LONGS,
        ) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => reply,
                Err(_) => {
                    self.finish(conn, atoms, target_window, false);
                    return XdndOutcome::Handled;
                }
            },
            Err(_) => {
                self.finish(conn, atoms, target_window, false);
                return XdndOutcome::Handled;
            }
        };
        // 普通传输和 INCR 握手都要求目标删除属性；INCR 源会据此开始发送第一块。
        let _ = conn.delete_property(target_window, atoms.xdnd_data);

        if reply.type_ == atoms.incr {
            self.phase = Phase::Incremental;
            self.buffer.clear();
            let _ = conn.flush();
            return XdndOutcome::Handled;
        }
        if reply.bytes_after != 0
            || reply.format != 8
            || reply.type_ != self.target_type
            || reply.value.len() > MAX_TRANSFER_BYTES
        {
            self.finish(conn, atoms, target_window, false);
            return XdndOutcome::Handled;
        }
        self.complete_payload(conn, atoms, target_window, reply.value)
    }

    /// 处理 INCR selection 的一个属性块。窗口必须订阅 `PROPERTY_CHANGE`。
    pub(crate) fn handle_property_notify(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        event: &PropertyNotifyEvent,
    ) -> XdndOutcome {
        if self.phase != Phase::Incremental
            || event.window != target_window
            || event.atom != atoms.xdnd_data
        {
            return XdndOutcome::Ignored;
        }
        if event.state != x11rb::protocol::xproto::Property::NEW_VALUE {
            return XdndOutcome::Handled;
        }
        let reply = match conn.get_property(
            true,
            target_window,
            atoms.xdnd_data,
            AtomEnum::ANY,
            0,
            MAX_PROPERTY_LONGS,
        ) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => reply,
                Err(_) => {
                    self.finish(conn, atoms, target_window, false);
                    return XdndOutcome::Handled;
                }
            },
            Err(_) => {
                self.finish(conn, atoms, target_window, false);
                return XdndOutcome::Handled;
            }
        };
        if reply.bytes_after != 0 {
            self.finish(conn, atoms, target_window, false);
            return XdndOutcome::Handled;
        }
        // INCR 用零长度属性表示传输结束，此时 type/format 可能为 NONE/0。
        if reply.value.is_empty() {
            let payload = std::mem::take(&mut self.buffer);
            return self.complete_payload(conn, atoms, target_window, payload);
        }
        if reply.format != 8 || reply.type_ != self.target_type || !self.append_chunk(&reply.value)
        {
            self.finish(conn, atoms, target_window, false);
        }
        XdndOutcome::Handled
    }

    /// 事件循环每帧调用一次；返回 true 表示刚刚中止了超时传输。
    pub(crate) fn poll_timeout(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        now: Instant,
    ) -> bool {
        if self.is_timed_out(now) {
            self.finish(conn, atoms, target_window, false);
            true
        } else {
            false
        }
    }

    fn handle_enter(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        data: [u32; 5],
    ) {
        if matches!(self.phase, Phase::AwaitingSelection | Phase::Incremental) {
            self.finish(conn, atoms, target_window, false);
        } else {
            self.reset();
        }
        let source = data[0];
        let offered_version = data[1] >> 24;
        if source == NONE || offered_version == 0 {
            return;
        }
        let target_type = if data[1] & 1 != 0 {
            read_supported_type(conn, atoms, source)
        } else {
            choose_type([data[2], data[3], data[4]], atoms.text_uri_list)
        };
        self.source = source;
        self.version = offered_version.min(XDND_VERSION);
        self.target_type = target_type;
        self.phase = Phase::Hover;
    }

    fn complete_payload(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        payload: Vec<u8>,
    ) -> XdndOutcome {
        let paths = parse_uri_list(&payload);
        let success = !paths.is_empty();
        self.finish(conn, atoms, target_window, success);
        if success {
            XdndOutcome::Dropped(paths)
        } else {
            XdndOutcome::Handled
        }
    }

    fn append_chunk(&mut self, chunk: &[u8]) -> bool {
        let Some(new_len) = self.buffer.len().checked_add(chunk.len()) else {
            return false;
        };
        if new_len > MAX_TRANSFER_BYTES {
            return false;
        }
        self.buffer.extend_from_slice(chunk);
        true
    }

    fn is_timed_out(&self, now: Instant) -> bool {
        matches!(self.phase, Phase::AwaitingSelection | Phase::Incremental)
            && self.deadline.is_some_and(|deadline| now >= deadline)
    }

    fn finish(
        &mut self,
        conn: &RustConnection,
        atoms: &XdndAtoms,
        target_window: Window,
        success: bool,
    ) {
        if self.source != NONE {
            send_finished(conn, atoms, target_window, self.source, success);
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.source = NONE;
        self.version = 0;
        self.target_type = NONE;
        self.phase = Phase::Idle;
        self.deadline = None;
        self.buffer.clear();
    }
}

fn intern(conn: &RustConnection, name: &[u8]) -> Result<Atom, String> {
    conn.intern_atom(false, name)
        .map_err(|error| format!("注册 X11 Atom 失败: {error}"))?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| format!("读取 X11 Atom 失败: {error}"))
}

fn choose_type(offered: impl IntoIterator<Item = Atom>, text_uri_list: Atom) -> Atom {
    offered
        .into_iter()
        .find(|atom| *atom == text_uri_list)
        .unwrap_or(NONE)
}

fn read_supported_type(conn: &RustConnection, atoms: &XdndAtoms, source: Window) -> Atom {
    let Ok(cookie) =
        conn.get_property(false, source, atoms.xdnd_type_list, AtomEnum::ATOM, 0, 1024)
    else {
        return NONE;
    };
    let Ok(reply) = cookie.reply() else {
        return NONE;
    };
    choose_type(reply.value32().into_iter().flatten(), atoms.text_uri_list)
}

fn send_status(
    conn: &RustConnection,
    atoms: &XdndAtoms,
    target: Window,
    source: Window,
    accepted: bool,
) {
    let flags = u32::from(accepted);
    let action = if accepted {
        atoms.xdnd_action_copy
    } else {
        NONE
    };
    let event =
        ClientMessageEvent::new(32, source, atoms.xdnd_status, [target, flags, 0, 0, action]);
    let _ = conn.send_event(false, source, EventMask::NO_EVENT, event);
    let _ = conn.flush();
}

fn send_finished(
    conn: &RustConnection,
    atoms: &XdndAtoms,
    target: Window,
    source: Window,
    success: bool,
) {
    let action = if success {
        atoms.xdnd_action_copy
    } else {
        NONE
    };
    let event = ClientMessageEvent::new(
        32,
        source,
        atoms.xdnd_finished,
        [target, u32::from(success), action, 0, 0],
    );
    let _ = conn.send_event(false, source, EventMask::NO_EVENT, event);
    let _ = conn.flush();
}

/// 把 `text/uri-list` 转成框架约定的本地绝对路径。
fn parse_uri_list(payload: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(payload) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| has_valid_percent_encoding(line))
        .filter_map(|line| Url::parse(line).ok())
        .filter(|url| url.scheme() == "file")
        .filter_map(|url| url.to_file_path().ok())
        .filter(|path| path.is_absolute())
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

fn has_valid_percent_encoding(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri列表支持注释空格中文和不同换行() {
        let payload = concat!(
            "# 文件管理器注释\r\n",
            "file:///tmp/a%20b.txt\r\n",
            "\n",
            "file:///tmp/%E4%B8%AD%E6%96%87.txt\n",
            "file://localhost/tmp/folder/\r\n",
        );
        assert_eq!(
            parse_uri_list(payload.as_bytes()),
            vec![
                "/tmp/a b.txt".to_string(),
                "/tmp/中文.txt".to_string(),
                "/tmp/folder/".to_string(),
            ]
        );
    }

    #[test]
    fn uri列表拒绝非文件远程主机相对路径和坏编码() {
        let payload = concat!(
            "https://example.com/a.txt\n",
            "file://remote-host/share/a.txt\n",
            "relative/path.txt\n",
            "file:///tmp/%ZZ.txt\n",
        );
        assert!(parse_uri_list(payload.as_bytes()).is_empty());
        assert!(parse_uri_list(&[0xff, 0xfe]).is_empty());
    }

    #[test]
    fn 类型选择只接受uri列表() {
        assert_eq!(choose_type([1, 7, 9], 7), 7);
        assert_eq!(choose_type([1, 2, 3], 7), NONE);
    }

    #[test]
    fn incr累计受八mib上限保护() {
        let mut target = XdndTarget::new();
        assert!(target.append_chunk(&vec![1; MAX_TRANSFER_BYTES]));
        assert!(!target.append_chunk(&[2]));
        assert_eq!(target.buffer.len(), MAX_TRANSFER_BYTES);
    }

    #[test]
    fn 只有等待selection和incr阶段会超时() {
        let now = Instant::now();
        let mut target = XdndTarget::new();
        target.deadline = Some(now);
        assert!(!target.is_timed_out(now));

        target.phase = Phase::AwaitingSelection;
        assert!(target.is_timed_out(now));
        target.phase = Phase::Incremental;
        assert!(target.is_timed_out(now));
        target.deadline = Some(now + Duration::from_millis(1));
        assert!(!target.is_timed_out(now));
    }

    #[test]
    fn reset清理完整拖放状态() {
        let now = Instant::now();
        let mut target = XdndTarget {
            source: 10,
            version: XDND_VERSION,
            target_type: 20,
            phase: Phase::Incremental,
            deadline: Some(now),
            buffer: vec![1, 2, 3],
        };
        target.reset();
        assert_eq!(target.source, NONE);
        assert_eq!(target.version, 0);
        assert_eq!(target.target_type, NONE);
        assert_eq!(target.phase, Phase::Idle);
        assert!(target.deadline.is_none());
        assert!(target.buffer.is_empty());
    }
}
