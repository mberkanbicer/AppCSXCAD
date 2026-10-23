//! AppCSXCAD entry point: winit + egui + wgpu GUI application for CSXCAD geometry viewing.

mod app;

use app::AppCsxCad;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowAttributes;

struct AppHandler {
    app: Option<AppCsxCad>,
}

impl ApplicationHandler<()> for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.is_some() {
            return;
        }

        let window = event_loop
            .create_window(
                WindowAttributes::default()
                    .with_title("AppCSXCAD")
                    .with_inner_size(winit::dpi::LogicalSize::new(1200.0, 800.0))
                    .with_min_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0)),
            )
            .unwrap();

        let mut egui_app = qcsxcad_egui::EguiApp::new();
        pollster::block_on(egui_app.set_window(window));

        let mut app = AppCsxCad::new(egui_app);
        let args: Vec<String> = std::env::args().collect();
        app.parse_command_line_args(&args);

        self.app = Some(app);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(app) = self.app.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                app.save_settings();
                _event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                app.handle_resize(size.width, size.height);
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = app.render() {
                    eprintln!("render error: {e}");
                    _event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app) = self.app.as_ref() {
            app.request_redraw();
        }
    }
}

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();
    let mut handler = AppHandler { app: None };
    event_loop.run_app(&mut handler).unwrap();
}
