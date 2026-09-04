//! README 中最小 XML 窗口的可执行版本。

use flexui::{Skin, Window, WindowCtx, WindowImpl};

struct HelloWindow;

impl WindowImpl for HelloWindow {
    fn skin(&self) -> Skin {
        Skin::xml(
            r#"
            <Window title="FlexUI" width="420" height="240">
              <VBox padding="24" spacing="16" align="center" justify="center">
                <Label text-verbatim="Hello from FlexUI" font-size="24" bold="true"/>
                <Button name="close" text-verbatim="Close" variant="primary"
                        width="120" height="40"/>
              </VBox>
            </Window>
            "#,
        )
    }

    fn on_click(&mut self, name: &str, ctx: &mut WindowCtx) {
        if name == "close" {
            ctx.close();
        }
    }
}

fn main() {
    Window::new(HelloWindow).center().run();
}
