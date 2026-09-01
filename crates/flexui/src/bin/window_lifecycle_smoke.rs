//! 三平台原生窗口生命周期冒烟测试。
//!
//! 该程序会创建真实窗口，通过 `MainProxy` 从后台线程请求关闭，并校验完整钩子顺序。

use std::{
    process,
    sync::{Arc, Mutex},
    time::Duration,
};

use flexui::{Skin, Window, WindowCtx, WindowImpl};

const EXPECTED: &[&str] = &[
    "before_init",
    "init",
    "initialized",
    "closing_vetoed",
    "closing",
    "closed",
];

struct LifecycleSmoke {
    events: Arc<Mutex<Vec<&'static str>>>,
    closing_attempts: usize,
}

impl LifecycleSmoke {
    fn record(&self, event: &'static str) {
        self.events.lock().expect("生命周期记录锁中毒").push(event);
    }
}

impl WindowImpl for LifecycleSmoke {
    fn skin(&self) -> Skin {
        Skin::xml(
            r#"
            <Window title="FlexUI Lifecycle Smoke" width="360" height="180">
              <Panel width="fill" height="fill" padding="24">
                <Label text-verbatim="FlexUI native window smoke test"
                       width="fill" height="fill" align="center" valign="center"/>
              </Panel>
            </Window>
            "#,
        )
    }

    fn on_before_init(&mut self, _ctx: &mut WindowCtx) {
        self.record("before_init");
    }

    fn on_init(&mut self, _ctx: &mut WindowCtx) {
        self.record("init");
    }

    fn on_initialized(&mut self, ctx: &mut WindowCtx) {
        self.record("initialized");
        let Some(proxy) = ctx.main_proxy() else {
            eprintln!("FLEXUI_WINDOW_LIFECYCLE_FAILED: MainProxy unavailable");
            process::exit(1);
        };
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            if !proxy.post(|ctx| ctx.close()) {
                eprintln!("FLEXUI_WINDOW_LIFECYCLE_FAILED: MainProxy closed too early");
                process::exit(1);
            }
        });
    }

    fn on_closing(&mut self, ctx: &mut WindowCtx) -> bool {
        self.closing_attempts += 1;
        if self.closing_attempts == 1 {
            self.record("closing_vetoed");
            let Some(proxy) = ctx.main_proxy() else {
                eprintln!("FLEXUI_WINDOW_LIFECYCLE_FAILED: MainProxy closed after veto");
                process::exit(1);
            };
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(80));
                if !proxy.post(|ctx| ctx.close()) {
                    eprintln!("FLEXUI_WINDOW_LIFECYCLE_FAILED: close retry was rejected");
                    process::exit(1);
                }
            });
            return false;
        }
        self.record("closing");
        true
    }

    fn on_closed(&mut self) {
        self.record("closed");
        let events = self.events.lock().expect("生命周期记录锁中毒");
        if events.as_slice() == EXPECTED {
            println!("FLEXUI_WINDOW_LIFECYCLE_OK: {}", events.join(" -> "));
            process::exit(0);
        }
        eprintln!(
            "FLEXUI_WINDOW_LIFECYCLE_FAILED: expected={EXPECTED:?}, actual={:?}",
            events.as_slice()
        );
        process::exit(1);
    }
}

fn main() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(15));
        eprintln!("FLEXUI_WINDOW_LIFECYCLE_FAILED: timed out after 15 seconds");
        process::exit(124);
    });

    Window::new(LifecycleSmoke {
        events: Arc::new(Mutex::new(Vec::new())),
        closing_attempts: 0,
    })
    .center()
    .run();
}
