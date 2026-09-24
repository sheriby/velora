use gpui::*;

struct Maksher;

impl Render for Maksher {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child("maksher")
    }
}

fn main() {
    Application::new().run(|cx| {
        let bounds = Bounds::centered(None, size(px(1080.0), px(720.0)), cx);
        cx.open_window(
            WindowOptions {
                app_id: Some("app.maksher.editor".to_string()),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            },
            |_, cx| cx.new(|_| Maksher),
        )
        .expect("failed to open maksher window");
    });
}
