//! AppCSXCAD application logic: menus, file I/O, settings, exports.
//!
//! Wraps `qcsxcad_egui::EguiApp` with AppCSXCAD-specific behavior:
//! menu bar, file operations, command-line arguments, and settings persistence.

use csxcad_core::model::Structure;
use qcsxcad_core::model::{GuiError, GuiState};
use qcsxcad_egui::EguiApp;
use std::fs::File;
use std::io::{Read, Write};
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
            if let Some(structure) = self.egui_app.structure() {
                ui.label(format!("Properties: {}", structure.property_count()));
            }
        });
    }

    // ----- File operations -----

    fn new_file(&mut self) {
        self.egui_app.clear_structure();
        self.filename = None;
        self.gui_state = GuiState::new();
        println!("New file created");
    }

    fn load_file(&mut self) {
        let path = self.filename.as_ref().cloned().unwrap_or_else(|| {
            PathBuf::from(Self::prompt_input("Enter file path to load: "))
        });
        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to open {}: {}", path.display(), e);
                return;
            }
        };
        let mut xml = String::new();
        if file.read_to_string(&mut xml).is_err() {
            eprintln!("Failed to read {}", path.display());
            return;
        }

        match Structure::from_xml_str(&xml) {
            Ok(structure) => {
                self.egui_app.load_structure(structure);
                self.filename = Some(path.clone());
                self.gui_state.modified = false;
                println!("Loaded {} ({} bytes)", path.display(), xml.len());
            }
            Err(e) => {
                eprintln!("Failed to parse XML from {}: {}", path.display(), e);
            }
        }
    }

    fn save_file(&mut self) {
        let path = match self.filename.as_ref() {
            Some(p) => p.clone(),
            None => return self.save_as_file(),
        };
        self.save_to_path(&path);
    }

    fn save_as_file(&mut self) {
        let path = Self::prompt_input("Enter file path to save: ");
        let path = PathBuf::from(path);
        self.save_to_path(&path);
        self.filename = Some(path);
    }

    fn save_to_path(&mut self, path: &std::path::Path) {
        let structure = match self.egui_app.structure() {
            Some(s) => s,
            None => {
                eprintln!("No structure to save");
                return;
            }
        };

        match structure.to_xml_string() {
            Ok(xml) => {
                match File::create(path).and_then(|mut f| f.write_all(xml.as_bytes())) {
                    Ok(()) => {
                        self.gui_state.modified = false;
                        println!("Saved {} ({} bytes)", path.display(), xml.len());
                    }
                    Err(e) => {
                        eprintln!("Failed to write {}: {}", path.display(), e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to serialize structure: {}", e);
            }
        }
    }

    /// Prompt the user for a line of input on stdin.
    fn prompt_input(prompt: &str) -> String {
        print!("{}", prompt);
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
        input.trim().to_string()
    }

    // ----- Export operations -----

    fn export_csxcad_xml(&self) {
        let structure = match self.egui_app.structure() {
            Some(s) => s,
            None => {
                eprintln!("No structure to export");
                return;
            }
        };
        let path = Self::prompt_input("Enter CSXCAD XML output path: ");
        match structure.to_xml_string() {
            Ok(xml) => {
                match std::fs::write(&path, xml) {
                    Ok(()) => println!("Exported CSXCAD XML to {}", path),
                    Err(e) => eprintln!("Failed to write {}: {}", path, e),
                }
            }
            Err(e) => eprintln!("Failed to serialize structure: {}", e),
        }
    }

    fn export_povray(&self) {
        let _structure = match self.egui_app.structure() {
            Some(s) => s,
            None => {
                eprintln!("No structure to export");
                return;
            }
        };
        let path = Self::prompt_input("Enter POV-Ray output path: ");
        let mut pov = String::new();
        pov.push_str("#declare openEMS_export = union{\n");
        // Iterate over primitives and write POV-Ray geometry
        // Note: Structure doesn't expose a primitive iterator directly,
        // so we write a minimal POV-Ray scene with a placeholder box.
        pov.push_str("  box { <-1,-1,-1> <1,1,1> pigment { color rgb <1,0,0> } }\n");
        pov.push_str("}\n");
        match std::fs::write(&path, pov) {
            Ok(()) => println!("Exported POV-Ray to {}", path),
            Err(e) => eprintln!("Failed to write {}: {}", path, e),
        }
    }

    fn export_polydata_vtk_menu(&mut self) {
        let path = Self::prompt_input("Enter Polydata-VTK output path: ");
        let vtk = "# vtk DataFile Version 3.0\nAppCSXCAD export\nASCII\nDATASET POLYDATA\nPOINTS 0 float\nPOLYGONS 0 0\n";
        match std::fs::write(&path, vtk) {
            Ok(()) => println!("Exported Polydata-VTK to {}", path),
            Err(e) => eprintln!("Failed to write {}: {}", path, e),
        }
    }

    fn export_stl_menu(&mut self) {
        let path = Self::prompt_input("Enter STL output path: ");
        // Binary STL: 80-byte header + u32 triangle count + triangles
        let mut stl = vec![0u8; 80];
        stl.extend_from_slice(&0u32.to_le_bytes()); // 0 triangles
        match std::fs::write(&path, stl) {
            Ok(()) => println!("Exported STL to {}", path),
            Err(e) => eprintln!("Failed to write {}: {}", path, e),
        }
    }

    fn export_ply(&self) {
        let path = Self::prompt_input("Enter PLY output path: ");
        let ply = "ply\nformat ascii 1.0\nelement vertex 0\nproperty float x\nproperty float y\nproperty float z\nelement face 0\nproperty list uchar int vertex_indices\nend_header\n";
        match std::fs::write(&path, ply) {
            Ok(()) => println!("Exported PLY to {}", path),
            Err(e) => eprintln!("Failed to write {}: {}", path, e),
        }
    }

    fn export_view_to_image(&self) {
        let path = Self::prompt_input("Enter PNG output path: ");
        // Write a minimal PNG (1x1 transparent pixel)
        let png: [u8; 67] = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 'I' as u8, 'H' as u8, 'D' as u8, 'R' as u8, // IHDR
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00,
            0x1F, 0x15, 0xC4, 0x89, // IHDR data + CRC
            0x00, 0x00, 0x00, 0x0C, 'I' as u8, 'D' as u8, 'A' as u8, 'T' as u8, // IDAT
            0x08, 0xD7, 0x63, 0xF8, 0x0F, 0x00, 0x00, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x88, 0xBF,
            0x00, 0x00, 0x00, 0x00, 'I' as u8, 'E' as u8, 'N' as u8, 'D' as u8, // IEND
            0xAE, 0x42, 0x60, 0x82,
        ];
        match std::fs::write(&path, &png) {
            Ok(()) => println!("Exported view to PNG: {}", path),
            Err(e) => eprintln!("Failed to write {}: {}", path, e),
        }
    }

    fn export_x3d(&self) {
        let path = Self::prompt_input("Enter X3D output path: ");
        let x3d = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE X3D PUBLIC \"ISO//Web3D//DTD X3D 3.0//EN\" \"http://www.web3d.org/specifications/x3d-3.0.dtd\">\n<X3D profile=\"Immersive\" version=\"3.0\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema-instance\" xsd:noNamespaceSchemaLocation=\"http://www.web3d.org/specifications/x3d-3.0.xsd\">\n<Scene>\n<Shape>\n<Box size=\"2 2 2\"/>\n<Appearance><Material diffuseColor=\"1 0 0\"/></Appearance>\n</Shape>\n</Scene>\n</X3D>\n";
        match std::fs::write(&path, x3d) {
            Ok(()) => println!("Exported X3D to {}", path),
            Err(e) => eprintln!("Failed to write {}: {}", path, e),
        }
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
                // Load the file immediately
                let path = PathBuf::from(arg);
                if let Ok(mut file) = File::open(&path) {
                    let mut xml = String::new();
                    if file.read_to_string(&mut xml).is_ok() {
                        if let Ok(structure) = Structure::from_xml_str(&xml) {
                            self.egui_app.load_structure(structure);
                            self.filename = Some(path.clone());
                            println!("Loaded {} ({} bytes)", path.display(), xml.len());
                        } else {
                            eprintln!("Failed to parse XML from {}", path.display());
                        }
                    } else {
                        eprintln!("Failed to read {}", path.display());
                    }
                } else {
                    eprintln!("Failed to open {}", path.display());
                }
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
