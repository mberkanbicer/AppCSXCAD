//! AppCSXCAD application logic: menus, file I/O, settings, exports.
//!
//! Wraps `qcsxcad_egui::EguiApp` with AppCSXCAD-specific behavior:
//! menu bar, file operations, command-line arguments, and settings persistence.

use qcsxcad_core::model::{GuiError, GuiState};
use qcsxcad_egui::EguiApp;
use std::path::PathBuf;
use winit::event::Event;

const APP_NAME: &str = "AppCSXCAD";

/// AppCSXCAD application: egui UI + wgpu 3D viewport with file I/O and settings.
#[derive(Default)]
pub struct AppCsxCad {
    egui_app: EguiApp,
    gui_state: GuiState,
    filename: Option<PathBuf>,
    edit_mode: bool,
    show_about: bool,
    export_polydata_vtk: Option<String>,
    export_stl: Option<String>,
    settings: AppSettings,
}

/// Persistent settings: window geometry and state.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    pub window_geometry: Option<Vec<u8>>,
    pub window_state: Option<Vec<u8>>,
}

impl AppCsxCad {
    /// Create a new AppCSXCAD application.
    pub fn new(egui_app: EguiApp) -> Self {
        let settings = AppSettings::load();
        Self {
            egui_app,
            gui_state: GuiState::new(),
            filename: None,
            edit_mode: true,
            show_about: false,
            export_polydata_vtk: None,
            export_stl: None,
            settings,
        }
    }

    /// Handle a winit event.
    pub fn handle_event(&mut self, event: &Event<()>) {
        self.egui_app.handle_event(event);
    }

    /// Handle window resize.
    pub fn handle_resize(&mut self, _width: u32, _height: u32) {
        // The wgpu surface resize is handled by EguiApp internally.
    }

    /// Render one frame.
    pub fn render(&mut self) -> Result<(), GuiError> {
        self.egui_app.render()
    }

    /// Build the egui UI for one frame.
    pub fn ui(&mut self, ctx: &egui::Context) {
        let _output = ctx.run_ui(egui::RawInput::default(), |ui| {
            self.menu_bar(ui);
            self.central_panel(ui);

            if self.show_about {
                egui::Window::new("About AppCSXCAD")
                    .open(&mut self.show_about)
                    .show(ctx, |ui| {
                        ui.label("AppCSXCAD");
                        ui.label("CSXCAD geometry viewer and editor");
                        ui.label("Ported to Rust with egui + wgpu");
                        ui.separator();
                        ui.label("Copyright (C) 2012-2015 Thorsten Liebig");
                        ui.label("GPL-3.0-or-later");
                    });
            }
        });
    }

    /// Request a redraw of the window.
    pub fn request_redraw(&self) {
        self.egui_app.request_redraw();
    }

    /// Menu bar: File and Info menus.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu_bar").show(ui, |ui| {
            egui::menu::MenuBar::default().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if self.edit_mode {
                        if ui.button("New").clicked() {
                            self.new_file();
                            ui.close();
                        }
                    }
                    if ui.button("Load…").clicked() {
                        self.load_file();
                        ui.close();
                    }
                    ui.separator();
                    if self.edit_mode {
                        if ui.button("Save").clicked() {
                            self.save_file();
                            ui.close();
                        }
                        if ui.button("Save As…").clicked() {
                            self.save_as_file();
                            ui.close();
                        }
                        ui.separator();
                    }
                    ui.menu_button("Export", |ui| {
                        if ui.button("CSXCAD XML").clicked() {
                            self.export_csxcad_xml();
                            ui.close();
                        }
                        if ui.button("Povray").clicked() {
                            self.export_povray();
                            ui.close();
                        }
                        if ui.button("Polydata-VTK").clicked() {
                            self.export_polydata_vtk_menu();
                            ui.close();
                        }
                        if ui.button("STL").clicked() {
                            self.export_stl_menu();
                            ui.close();
                        }
                        if ui.button("PLY").clicked() {
                            self.export_ply();
                            ui.close();
                        }
                        if ui.button("3D view to PNG").clicked() {
                            self.export_view_to_image();
                            ui.close();
                        }
                        if ui.button("X3D").clicked() {
                            self.export_x3d();
                            ui.close();
                        }
                    });
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        self.save_settings();
                        std::process::exit(0);
                    }
                });
                ui.menu_button("Info", |ui| {
                    if ui.button("About").clicked() {
                        self.show_about = true;
                        ui.close();
                    }
                });
            });
        });
    }

    /// Central panel: placeholder content.
    fn central_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("AppCSXCAD");
            if self.filename.is_none() {
                ui.label("No structure loaded. Use File → Load to open a CSXCAD XML file.");
            } else {
                ui.label(format!("Loaded: {:?}", self.filename.as_deref().map(|p| p.display().to_string()).unwrap_or_default()));
            }
            ui.separator();
            ui.label("(File I/O and 3D viewport rendering are stubs pending csxcad-core serialization API.)");
        });
    }

    // ----- File operations -----

    fn new_file(&mut self) {
        self.filename = None;
        self.gui_state = GuiState::new();
        eprintln!("New file (stub)");
    }

    fn load_file(&mut self) {
        eprintln!("Load file (stub) - csxcad-core serialization API not yet public");
    }

    fn save_file(&mut self) {
        match self.filename.as_ref() {
            Some(_) => {
                eprintln!("Save file (stub)");
            }
            None => self.save_as_file(),
        }
    }

    fn save_as_file(&mut self) {
        eprintln!("Save As (stub)");
    }

    // ----- Export operations -----

    fn export_csxcad_xml(&self) {
        eprintln!("Export CSXCAD XML (stub)");
    }

    fn export_povray(&self) {
        eprintln!("Povray export (stub)");
    }

    fn export_polydata_vtk_menu(&mut self) {
        eprintln!("Export Polydata-VTK (stub)");
    }

    fn export_stl_menu(&mut self) {
        eprintln!("Export STL (stub)");
    }

    fn export_ply(&self) {
        eprintln!("PLY export (stub)");
    }

    fn export_view_to_image(&self) {
        eprintln!("Export view to PNG (stub)");
    }

    fn export_x3d(&self) {
        eprintln!("X3D export (stub)");
    }

    // ----- Command-line argument parsing -----

    /// Parse command-line arguments and set up export operations.
    pub fn parse_command_line_args(&mut self, args: &[String]) {
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with("--") {
                let parts: Vec<&str> = arg.splitn(2, '=').collect();
                match parts[0] {
                    "--export-polydata-vtk" => {
                        if parts.len() > 1 {
                            self.export_polydata_vtk = Some(parts[1].to_string());
                        } else if i + 1 < args.len() {
                            self.export_polydata_vtk = Some(args[i + 1].to_string());
                            i += 1;
                        }
                    }
                    "--export-stl" => {
                        if parts.len() > 1 {
                            self.export_stl = Some(parts[1].to_string());
                        } else if i + 1 < args.len() {
                            self.export_stl = Some(args[i + 1].to_string());
                            i += 1;
                        }
                    }
                    _ => {
                        eprintln!("Unknown argument: {arg}");
                    }
                }
            } else if !arg.starts_with('-') {
                // Positional argument: treat as file to load.
                self.filename = Some(std::path::PathBuf::from(arg));
                eprintln!("Loading file: {}", arg);
            }
            i += 1;
        }
    }

    /// Save settings on shutdown.
    pub fn save_settings(&self) {
        self.settings.save();
    }

    /// Load settings on startup.
    pub fn load_settings(&mut self) {
        self.settings = AppSettings::load();
    }

    /// Get the window title.
    pub fn title(&self) -> String {
        if self.edit_mode {
            APP_NAME.to_string()
        } else {
            format!("{} - {}", APP_NAME, "View Mode")
        }
    }
}

impl AppSettings {
    /// Get the settings file path.
    fn path() -> PathBuf {
        directories::ProjectDirs::from("", "", APP_NAME)
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| std::env::temp_dir())
            .join("settings.json")
    }

    /// Load settings from disk.
    fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        }
    }

    /// Save settings to disk.
    fn save(&self) {
        if let Some(parent) = Self::path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), json);
        }
    }
}
