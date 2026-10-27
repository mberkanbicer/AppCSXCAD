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

        let settings = app::AppSettings::load();
        let (width, height) = settings.window_size();
        let window = match event_loop.create_window(
            WindowAttributes::default()
                .with_title("AppCSXCAD")
                .with_inner_size(winit::dpi::LogicalSize::new(
                    f64::from(width),
                    f64::from(height),
                ))
                .with_min_inner_size(winit::dpi::LogicalSize::new(400.0, 300.0)),
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("AppCSXCAD: could not create the window: {error}");
                eprintln!("A graphical display is required to run the application.");
                std::process::exit(1);
            }
        };

        let mut egui_app = qcsxcad_egui::EguiApp::new();
        pollster::block_on(egui_app.set_window(window));

        let mut app = AppCsxCad::new(egui_app);
        // skip the program name; the native application only parses its own
        // arguments
        let args: Vec<String> = std::env::args().skip(1).collect();
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

/// Version reported by `--version`.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Usage text reported by `--help`.
const USAGE: &str = "\
Usage: appcsxcad [OPTIONS] [FILE]

Open a CSXCAD geometry file (XML). Without a file the application starts with
an empty structure.

Options:
  -h, --help       Print this help text and exit
  -V, --version    Print the version and exit
";

fn main() {
    env_logger::init();

    // Resolve the informational arguments before creating the event loop so
    // they work on headless systems (native AppCSXCAD prints the same).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("AppCSXCAD {VERSION}");
        return;
    }

    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("AppCSXCAD: could not create the window event loop: {error}");
            eprintln!("A graphical display is required to run the application.");
            std::process::exit(1);
        }
    };
    let mut handler = AppHandler { app: None };
    if let Err(error) = event_loop.run_app(&mut handler) {
        eprintln!("AppCSXCAD: event loop error: {error}");
        std::process::exit(1);
    }
}
