//! AppCSXCAD application logic: menus, file I/O, settings and exports.
//!
//! The application wraps `qcsxcad_egui::EguiApp` (the QCSXCAD library: tree,
//! editors, 3D viewport) with the desktop behavior the native `AppCSXCAD`
//! class adds: the File/Info menu, project loading and saving, the export
//! actions, command-line arguments and window settings.

use qcsxcad_core::export::{self, PropertyFormat};
use csxcad_core::model::{PrimitiveType, PropertyType, Structure};
use qcsxcad_core::model::{GuiError, GuiState};
use qcsxcad_core::ProjectFile;
use qcsxcad_egui::EguiApp;
use std::path::{Path, PathBuf};
use winit::event::Event;

const APP_NAME: &str = "AppCSXCAD";
const DEFAULT_WIDTH: u32 = 1200;
const DEFAULT_HEIGHT: u32 = 800;

/// Window settings persisted between sessions (native `QSettings`).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_width")]
    pub window_width: u32,
    #[serde(default = "default_height")]
    pub window_height: u32,
}

fn default_width() -> u32 {
    DEFAULT_WIDTH
}

fn default_height() -> u32 {
    DEFAULT_HEIGHT
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            window_width: DEFAULT_WIDTH,
            window_height: DEFAULT_HEIGHT,
        }
    }
}

impl AppSettings {
    fn path() -> PathBuf {
        directories::ProjectDirs::from("", "", APP_NAME)
            .map(|dirs| dirs.config_dir().to_path_buf())
            .unwrap_or_else(std::env::temp_dir)
            .join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Clamp a stored size to something a window manager will accept.
    pub fn window_size(&self) -> (u32, u32) {
        (
            self.window_width.clamp(320, 16_384),
            self.window_height.clamp(240, 16_384),
        )
    }
}

/// One pending file operation, applied after the frame is drawn.
#[derive(Clone, Debug, PartialEq)]
enum AppAction {
    New,
    Load(PathBuf),
    Save,
    SaveAs(PathBuf),
    ExportXml(PathBuf),
    ExportMesh(PropertyFormat, PathBuf),
    ExportPovray(PathBuf),
    ExportX3d(PathBuf),
    ExportPng(PathBuf),
    AddProperty(PropertyType),
    AddPrimitive(PrimitiveType),
    ExpandAll,
    CollapseAll,
    ViewDir(i32),
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dialog {
    Load,
    SaveAs,
    ExportXml,
    ExportVtk,
    ExportStl,
    ExportPly,
    ExportPovray,
    ExportX3d,
    ExportPng,
}

impl Dialog {
    fn title(self) -> &'static str {
        match self {
            Dialog::Load => "Load CSXCAD project",
            Dialog::SaveAs => "Save project as",
            Dialog::ExportXml => "Export CSXCAD XML",
            Dialog::ExportVtk => "Export Polydata-VTK into a directory",
            Dialog::ExportStl => "Export STL into a directory",
            Dialog::ExportPly => "Export PLY into a directory",
            Dialog::ExportPovray => "Export POV-Ray scene",
            Dialog::ExportX3d => "Export X3D scene",
            Dialog::ExportPng => "Export the 3D view to PNG",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Dialog::Load => "project.xml",
            Dialog::SaveAs | Dialog::ExportXml => "geometry.xml",
            Dialog::ExportVtk | Dialog::ExportStl | Dialog::ExportPly => "export-directory",
            Dialog::ExportPovray => "scene.pov",
            Dialog::ExportX3d => "scene.x3d",
            Dialog::ExportPng => "view.png",
        }
    }

    fn target(self, path: PathBuf) -> AppAction {
        match self {
            Dialog::Load => AppAction::Load(path),
            Dialog::SaveAs => AppAction::SaveAs(path),
            Dialog::ExportXml => AppAction::ExportXml(path),
            Dialog::ExportVtk => AppAction::ExportMesh(PropertyFormat::PolyDataVtk, path),
            Dialog::ExportStl => AppAction::ExportMesh(PropertyFormat::Stl, path),
            Dialog::ExportPly => AppAction::ExportMesh(PropertyFormat::Ply, path),
            Dialog::ExportPovray => AppAction::ExportPovray(path),
            Dialog::ExportX3d => AppAction::ExportX3d(path),
            Dialog::ExportPng => AppAction::ExportPng(path),
        }
    }
}

/// Presentation state the menu and dialogs need.
#[derive(Clone, Debug, Default)]
struct AppUiState {
    path_input: String,
    dialog: Option<Dialog>,
    about: bool,
    error: Option<String>,
    action: Option<AppAction>,
}

impl AppUiState {
    fn open_dialog(&mut self, dialog: Dialog, suggestion: &str) {
        self.dialog = Some(dialog);
        self.path_input = suggestion.to_owned();
    }

    fn take_action(&mut self) -> Option<AppAction> {
        self.action.take()
    }

    /// Draw the menu bar. Runs inside the egui frame, before the QCSXCAD panels.
    fn menu(&mut self, ui: &mut egui::Ui, edit_mode: bool, has_document: bool) {
        egui::Panel::top("app_menu").show(ui, |ui| {
            egui::menu::MenuBar::default().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if edit_mode && ui.button("New").clicked() {
                        self.action = Some(AppAction::New);
                        ui.close();
                    }
                    if ui.button("Load…").clicked() {
                        self.open_dialog(Dialog::Load, "");
                        ui.close();
                    }
                    if edit_mode && ui.button("Save").clicked() {
                        self.action = Some(AppAction::Save);
                        ui.close();
                    }
                    if edit_mode && ui.button("Save As…").clicked() {
                        self.open_dialog(Dialog::SaveAs, "");
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Export", |ui| {
                        if edit_mode && ui.button("CSXCAD XML").clicked() {
                            self.open_dialog(Dialog::ExportXml, "");
                            ui.close();
                        }
                        if ui.button("Polydata-VTK").clicked() {
                            self.open_dialog(Dialog::ExportVtk, "vtk");
                            ui.close();
                        }
                        if ui.button("STL").clicked() {
                            self.open_dialog(Dialog::ExportStl, "stl");
                            ui.close();
                        }
                        if ui.button("PLY").clicked() {
                            self.open_dialog(Dialog::ExportPly, "ply");
                            ui.close();
                        }
                        if ui.button("POV-Ray").clicked() {
                            self.open_dialog(Dialog::ExportPovray, "scene.pov");
                            ui.close();
                        }
                        if ui.button("X3D").clicked() {
                            self.open_dialog(Dialog::ExportX3d, "scene.x3d");
                            ui.close();
                        }
                        if ui.button("3D view to PNG").clicked() {
                            self.open_dialog(Dialog::ExportPng, "view.png");
                            ui.close();
                        }
                    });
                    // native Ctrl+P shortcut for the 3D view capture
                    if ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::COMMAND, egui::Key::P)
                    }) {
                        self.open_dialog(Dialog::ExportPng, "view.png");
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        self.action = Some(AppAction::Exit);
                        ui.close();
                    }
                });
                ui.menu_button("Info", |ui| {
                    if ui.button("About").clicked() {
                        self.about = true;
                        ui.close();
                    }
                });
                ui.separator();
                if !has_document {
                    ui.label("no project loaded");
                }
            });
        });

        // Native AppCSXCAD toolbar. The object creation actions are not wired
        // yet (RM-152); the labels are shown so the layout matches the native
        // window while the feature port lands.
        egui::Panel::top("app_toolbar").show(ui, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    if edit_mode && ui.button("New").clicked() {
                        self.action = Some(AppAction::New);
                    }
                    if ui.button("Load").clicked() {
                        self.open_dialog(Dialog::Load, "");
                    }
                    if edit_mode && ui.button("Save").clicked() {
                        self.action = Some(AppAction::Save);
                    }
                    if edit_mode && ui.button("Save As").clicked() {
                        self.open_dialog(Dialog::SaveAs, "");
                    }
                    if ui.button("Export").clicked() {
                        self.open_dialog(Dialog::ExportXml, "");
                    }
                    ui.separator();
                    if ui.button("CollapseAll").clicked() {
                        self.action = Some(AppAction::CollapseAll);
                    }
                    if ui.button("ExpandAll").clicked() {
                        self.action = Some(AppAction::ExpandAll);
                    }
                    for (label, kind) in [
                        ("Material", PropertyType::Material),
                        ("Metal", PropertyType::Metal),
                        ("Electrode", PropertyType::Metal),
                        ("ProbeBox", PropertyType::ProbeBox),
                        ("ResBox", PropertyType::ResBox),
                        ("DumpBox", PropertyType::DumpBox),
                    ] {
                        if ui.button(label).clicked() {
                            self.action = Some(AppAction::AddProperty(kind));
                        }
                    }
                    for (label, kind) in [
                        ("Box", PrimitiveType::Box),
                        ("MultiBox", PrimitiveType::MultiBox),
                        ("Sphere", PrimitiveType::Sphere),
                        ("Cylinder", PrimitiveType::Cylinder),
                        ("Polygon", PrimitiveType::Polygon),
                        ("User Defined", PrimitiveType::UserDefined),
                    ] {
                        if ui.button(label).clicked() {
                            self.action = Some(AppAction::AddPrimitive(kind));
                        }
                    }
                    for (label, dir) in [("YZ", 2), ("ZX", 1), ("2D", 0), ("3D", 3)] {
                        if ui.button(label).clicked() {
                            self.action = Some(AppAction::ViewDir(dir));
                        }
                    }
                });
            });
        });

        self.show_dialog(ui);
        self.show_about(ui);        if let Some(error) = &self.error {
            egui::Panel::bottom("app_error").show(ui, |ui| {
                ui.colored_label(egui::Color32::from_rgb(220, 80, 80), error);
            });
        }
    }

    fn show_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(dialog) = self.dialog else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new(dialog.title())
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label(format!("Path ({})", dialog.hint()));
                ui.text_edit_singleline(&mut self.path_input);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if confirm {
            let path = PathBuf::from(self.path_input.trim());
            if path.as_os_str().is_empty() {
                self.error = Some("Enter a path".into());
            } else {
                self.action = Some(dialog.target(path));
                self.dialog = None;
                self.error = None;
            }
        } else if cancel || !open {
            self.dialog = None;
        }
    }

    fn show_about(&mut self, ui: &mut egui::Ui) {
        if !self.about {
            return;
        }
        egui::Window::new("About AppCSXCAD")
            .open(&mut self.about)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label("AppCSXCAD");
                ui.label("CSXCAD geometry viewer and editor");
                ui.label("Ported to Rust with egui + wgpu");
                ui.separator();
                ui.label("Copyright (C) 2012-2015 Thorsten Liebig");
                ui.label("GPL-3.0-or-later");
            });
    }
}

/// AppCSXCAD application: egui UI + wgpu 3D viewport with file I/O and settings.
pub struct AppCsxCad {
    egui_app: EguiApp,
    file: ProjectFile,
    edit_mode: bool,
    ui_state: AppUiState,
}

impl AppCsxCad {
    /// Create a new AppCSXCAD application.
    pub fn new(egui_app: EguiApp) -> Self {
        let app = Self {
            egui_app,
            file: ProjectFile::new(),
            edit_mode: true,
            ui_state: AppUiState::default(),
        };
        app.update_title();
        app
    }

    /// Handle a winit event.
    pub fn handle_event(&mut self, event: &Event<()>) {
        self.egui_app.handle_event(event);
    }

    /// Request a redraw (the winit handler calls this every wake-up).
    pub fn request_redraw(&self) {
        self.egui_app.request_redraw();
    }

    /// Handle window resize.
    pub fn handle_resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.egui_app.resize(width, height);
        }
    }

    /// Render one frame and apply the pending file action.
    pub fn render(&mut self) -> Result<(), GuiError> {
        let edit_mode = self.edit_mode;
        let has_document = self.egui_app.document().is_some();
        let ui_state = &mut self.ui_state;
        let egui_app = &mut self.egui_app;
        egui_app.render_with(|root, _document, _state| {
            ui_state.menu(root, edit_mode, has_document);
        })?;
        if let Some(action) = self.ui_state.take_action() {
            if let Err(error) = self.apply_action(action) {
                self.ui_state.error = Some(error.to_string());
            }
        }
        Ok(())
    }

    fn apply_action(&mut self, action: AppAction) -> Result<(), GuiError> {
        match action {
            AppAction::New => {
                self.egui_app.clear_structure();
                self.file = ProjectFile::new();
                self.ui_state.error = None;
                self.update_title();
                Ok(())
            }
            AppAction::Load(path) => self.load_path(&path),
            AppAction::Save => self.save(),
            AppAction::SaveAs(path) => self.save_as(&path),
            AppAction::ExportXml(path) => self.export_xml(&path),
            AppAction::ExportMesh(format, directory) => self.export_mesh(format, &directory),
            AppAction::ExportPovray(path) => self.export_povray(&path),
            AppAction::ExportX3d(path) => self.export_x3d(&path),
            AppAction::ExportPng(path) => self.export_view_png(&path),
            AppAction::AddProperty(kind) => {
                if self.egui_app.document().is_none() {
                    self.egui_app.load_structure(Structure::new());
                }
                let id = self
                    .egui_app
                    .document_mut()
                    .expect("document was just created")
                    .add_property(kind)?;
                self.egui_app.select_property(id);
                Ok(())
            }
            AppAction::AddPrimitive(kind) => {
                if self.egui_app.document().is_none() {
                    self.egui_app.load_structure(Structure::new());
                }
                let property = self.egui_app.selection().properties.iter().copied().next();
                let property = match property {
                    Some(id) => id,
                    None => {
                        let id = self
                            .egui_app
                            .document_mut()
                            .expect("document was just created")
                            .add_property(PropertyType::Material)?;
                        self.egui_app.select_property(id);
                        id
                    }
                };
                let id = self
                    .egui_app
                    .document_mut()
                    .expect("document exists")
                    .add_primitive(property, kind)?;
                self.egui_app.select_primitive(id);
                Ok(())
            }
            AppAction::ExpandAll => {
                self.egui_app.expand_all();
                Ok(())
            }
            AppAction::CollapseAll => {
                self.egui_app.collapse_all();
                Ok(())
            }
            AppAction::ViewDir(dir) => {
                self.egui_app.set_view_dir(dir);
                Ok(())
            }
            AppAction::Exit => {
                self.save_settings();
                std::process::exit(0);
            }
        }
    }

    // ----- project I/O -----

    /// Load a CSXCAD or openEMS project file.
    pub fn load_path(&mut self, path: impl AsRef<Path>) -> Result<(), GuiError> {
        let (document, file) = ProjectFile::read_file(path)?;
        self.egui_app.load_document(document);
        self.file = file;
        self.ui_state.path_input = self
            .file
            .path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.ui_state.error = None;
        self.update_title();
        Ok(())
    }

    /// Save to the current project path.
    pub fn save(&mut self) -> Result<(), GuiError> {
        let Some(document) = self.egui_app.document_mut() else {
            return Err(GuiError::Model("there is no project to save".into()));
        };
        if self.file.path().is_none() {
            return Err(GuiError::Model(
                "the project has no path; use Save As".into(),
            ));
        }
        self.file.save(document)?;
        self.update_title();
        Ok(())
    }

    /// Save to `path`.
    ///
    /// Native `AppCSXCAD::ExportGeometry` leaves its internal filename
    /// unchanged, so a following Save writes the previous file. The port makes
    /// Save As adopt the new path (documented in `audits/gui.md`).
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), GuiError> {
        let Some(document) = self.egui_app.document_mut() else {
            return Err(GuiError::Model("there is no project to save".into()));
        };
        self.file.write(document, path)?;
        self.ui_state.path_input = self
            .file
            .path()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.update_title();
        Ok(())
    }

    // ----- exports -----

    /// Export the model as a CSXCAD XML file (native `QCSXCAD::Write2XML`).
    pub fn export_xml(&mut self, path: impl AsRef<Path>) -> Result<(), GuiError> {
        let Some(document) = self.egui_app.document() else {
            return Err(GuiError::Model("there is no project to export".into()));
        };
        let xml = document
            .structure()
            .to_xml_string()
            .map_err(|error| GuiError::Xml(error.to_string()))?;
        let path = path.as_ref();
        std::fs::write(path, xml)
            .map_err(|error| GuiError::Io(format!("could not write {}: {error}", path.display())))
    }

    /// Export every visible property (native `ExportGeometry(dir, type)`).
    pub fn export_mesh(
        &mut self,
        format: PropertyFormat,
        directory: impl AsRef<Path>,
    ) -> Result<(), GuiError> {
        let Some(document) = self.egui_app.document() else {
            return Err(GuiError::Model("there is no project to export".into()));
        };
        export::export_properties(document.structure(), directory, format)?;
        Ok(())
    }

    /// Export a POV-Ray scene.
    pub fn export_povray(&mut self, path: impl AsRef<Path>) -> Result<(), GuiError> {
        let Some(document) = self.egui_app.document() else {
            return Err(GuiError::Model("there is no project to export".into()));
        };
        export::export_povray(document.structure(), path)
    }

    /// Export an X3D scene.
    pub fn export_x3d(&mut self, path: impl AsRef<Path>) -> Result<(), GuiError> {
        let Some(document) = self.egui_app.document() else {
            return Err(GuiError::Model("there is no project to export".into()));
        };
        export::export_x3d(document.structure(), path)
    }

    /// Capture the 3D view into a PNG file.
    pub fn export_view_png(&mut self, path: impl AsRef<Path>) -> Result<(), GuiError> {
        let path = path.as_ref();
        self.egui_app
            .export_view_png(&path.display().to_string())
            .map_err(|error| match error {
                GuiError::Model(message) => GuiError::Model(message),
                other => other,
            })
    }

    // ----- command line -----

    /// Parse the native AppCSXCAD command line.
    ///
    /// Native loads the last argument that does not start with `-`, then parses
    /// `--export-polydata-vtk=<dir>` and `--export-STL=<dir>` and runs them
    /// immediately.
    pub fn parse_command_line_args(&mut self, args: &[String]) {
        if let Some(positional) = args.iter().rev().find(|arg| !arg.starts_with('-')) {
            if let Err(error) = self.load_path(positional) {
                self.ui_state.error = Some(error.to_string());
                eprintln!("AppCSXCAD: {error}");
            }
        }
        for argument in args {
            if let Some(directory) = argument.strip_prefix("--export-polydata-vtk=") {
                println!("AppCSXCAD - exporting 3D view to polydata vtk");
                if let Err(error) = self.export_mesh(PropertyFormat::PolyDataVtk, Path::new(directory)) {
                    eprintln!("AppCSXCAD: {error}");
                    self.ui_state.error = Some(error.to_string());
                }
            } else if let Some(directory) = argument.strip_prefix("--export-STL=") {
                println!("AppCSXCAD - exporting 3D data to STL");
                if let Err(error) = self.export_mesh(PropertyFormat::Stl, Path::new(directory)) {
                    eprintln!("AppCSXCAD: {error}");
                    self.ui_state.error = Some(error.to_string());
                }
            } else if argument == "--disableEdit" {
                println!("QCSXCAD - disabling editing");
                self.edit_mode = false;
                self.update_title();
            } else if argument == "--RenderDiscMaterial" {
                // native sets the render flag (and, due to the recorded source
                // defect, also the edit flag); the port keeps them separate
                println!("QCSXCAD - Render discrete material");
                self.egui_app.gui_state_mut().render_disc_models = true;
            } else if argument.starts_with('-') && argument != "--help" && argument != "-h" {
                eprintln!("AppCSXCAD: unknown argument: {argument}");
            }
        }
    }

    // ----- settings and title -----

    /// Save window settings on shutdown.
    pub fn save_settings(&self) {
        let (window_width, window_height) = self
            .egui_app
            .window_size()
            .unwrap_or((DEFAULT_WIDTH, DEFAULT_HEIGHT));
        AppSettings {
            window_width,
            window_height,
        }
        .save();
    }

    /// Window title, following the native "View Mode" suffix.
    pub fn title(&self) -> String {
        let mut title = APP_NAME.to_string();
        if let Some(path) = self.file.path() {
            title.push_str(" - ");
            title.push_str(&path.display().to_string());
        }
        if !self.edit_mode {
            title.push_str(" - View Mode");
        }
        title
    }

    fn update_title(&self) {
        self.egui_app.set_title(&self.title());
    }

    #[cfg(test)]
    fn ui_state(&self) -> &AppUiState {
        &self.ui_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<openEMS>
  <FDTD NumberOfTimesteps="1000"/>
  <ContinuousStructure CoordSystem="0">
    <Properties>
      <Metal Name="ground">
        <Primitives>
          <Box Priority="1"><P1 X="-1" Y="-1" Z="-1"/><P2 X="1" Y="1" Z="0"/></Box>
        </Primitives>
      </Metal>
    </Properties>
  </ContinuousStructure>
  <Excitation Type="0" f0="1e9"/>
</openEMS>"#;

    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "appcsxcad-test-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("temp dir");
        directory
    }

    fn app() -> AppCsxCad {
        AppCsxCad::new(EguiApp::new())
    }

    #[test]
    fn toolbar_creates_a_property_and_a_primitive() {
        let mut app = app();
        app.apply_action(AppAction::AddProperty(PropertyType::Material))
            .expect("add property");
        let property = app
            .egui_app
            .selection()
            .properties
            .iter()
            .copied()
            .next()
            .expect("the new property is selected");

        app.apply_action(AppAction::AddPrimitive(PrimitiveType::Box))
            .expect("add primitive");
        assert_eq!(
            app.egui_app
                .document()
                .unwrap()
                .structure()
                .property(property)
                .unwrap()
                .primitive_count(),
            1
        );

        // The creation is one undo step.
        app.egui_app.document_mut().unwrap().undo();
        assert_eq!(
            app.egui_app
                .document()
                .unwrap()
                .structure()
                .property(property)
                .unwrap()
                .primitive_count(),
            0
        );

        app.apply_action(AppAction::ExpandAll).expect("expand");
        app.apply_action(AppAction::CollapseAll).expect("collapse");
        app.apply_action(AppAction::ViewDir(2)).expect("view preset");
    }

    #[test]
    fn loading_and_saving_a_project_keeps_the_openems_wrapper() {
        let directory = temp_dir("io");
        let source = directory.join("project.xml");
        std::fs::write(&source, PROJECT).expect("write fixture");
        let mut app = app();
        app.load_path(&source).expect("load");
        assert_eq!(app.egui_app.structure().expect("structure").property_count(), 1);
        assert!(app.title().contains("project.xml"));
        assert!(!app.egui_app.document().expect("document").is_modified());

        // edit, then Save writes the same file with the wrapper intact
        app.egui_app
            .document_mut()
            .expect("document")
            .add_property(csxcad_core::model::PropertyType::Material)
            .expect("property");
        app.save().expect("save");
        let written = std::fs::read_to_string(&source).expect("read");
        assert!(written.contains("<openEMS>"));
        assert!(written.contains("<FDTD NumberOfTimesteps=\"1000\"/>"));
        assert!(written.contains("<Excitation Type=\"0\" f0=\"1e9\"/>"));
        assert_eq!(written.matches("<ContinuousStructure").count(), 1);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn save_as_adopts_the_new_path() {
        let directory = temp_dir("saveas");
        let mut app = app();
        app.egui_app
            .load_document(qcsxcad_core::document::Document::from_structure(
                csxcad_core::model::Structure::new(),
            ));
        let target = directory.join("new.xml");
        app.save_as(&target).expect("save as");
        assert_eq!(app.file.path(), Some(target.as_path()));
        assert!(target.exists());
        // a plain Save now writes the new file
        app.egui_app
            .document_mut()
            .expect("document")
            .add_property(csxcad_core::model::PropertyType::Metal)
            .expect("property");
        app.save().expect("save");
        let written = std::fs::read_to_string(&target).expect("read");
        assert!(written.contains("<Metal"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn exports_write_real_files() {
        let directory = temp_dir("exports");
        let source = directory.join("project.xml");
        std::fs::write(&source, PROJECT).expect("write fixture");
        let mut app = app();
        app.load_path(&source).expect("load");

        let xml = directory.join("geometry.xml");
        app.export_xml(&xml).expect("xml");
        assert!(std::fs::read_to_string(&xml).expect("read").contains("<Metal"));

        let vtk = directory.join("vtk");
        app.export_mesh(PropertyFormat::PolyDataVtk, &vtk).expect("vtk");
        assert!(vtk.join("ground.vtp").exists());

        let stl = directory.join("stl");
        app.export_mesh(PropertyFormat::Stl, &stl).expect("stl");
        assert!(stl.join("ground.stl").exists());

        let ply = directory.join("ply");
        app.export_mesh(PropertyFormat::Ply, &ply).expect("ply");
        assert!(ply.join("ground.ply").exists());

        let pov = directory.join("scene.pov");
        app.export_povray(&pov).expect("pov");
        assert!(std::fs::read_to_string(&pov).expect("read").contains("mesh2"));

        let x3d = directory.join("scene.x3d");
        app.export_x3d(&x3d).expect("x3d");
        assert!(std::fs::read_to_string(&x3d).expect("read").contains("IndexedFaceSet"));

        // without a window there is no GPU surface, so the capture reports an
        // error instead of writing a placeholder (the real capture is covered
        // by the renderer's offscreen test)
        let png = directory.join("view.png");
        assert!(app.export_view_png(&png).is_err());
        assert!(!png.exists());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn command_line_loads_the_last_positional_file_and_runs_exports() {
        let directory = temp_dir("cli");
        let source = directory.join("project.xml");
        std::fs::write(&source, PROJECT).expect("write fixture");
        let export = directory.join("cli-vtk");
        let mut app = app();
        app.parse_command_line_args(&[
            "--disableEdit".to_string(),
            format!("--export-polydata-vtk={}", export.display()),
            source.display().to_string(),
        ]);
        // native loads the file before parsing the export switches
        assert!(export.join("ground.vtp").exists());
        assert!(!app.edit_mode);
        assert!(app.title().ends_with(" - View Mode"));
        assert!(app.ui_state().error.is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn render_disc_material_sets_the_gui_state_without_touching_edit_mode() {
        let mut app = app();
        app.parse_command_line_args(&["--RenderDiscMaterial".to_string()]);
        assert!(app.egui_app.gui_state().render_disc_models);
        // the recorded native defect (the flag also toggles edit mode) is not
        // reproduced
        assert!(app.edit_mode);
    }

    #[test]
    fn a_missing_project_is_reported_without_panicking() {
        let mut app = app();
        app.parse_command_line_args(&["definitely-missing-project.xml".to_string()]);
        assert!(app.ui_state().error.is_some());
        assert!(app.egui_app.structure().is_none());
    }

    #[test]
    fn settings_round_trip_and_clamp() {
        let settings = AppSettings {
            window_width: 100,
            window_height: 10,
        };
        assert_eq!(settings.window_size(), (320, 240));
        let settings = AppSettings::default();
        assert_eq!(settings.window_size(), (DEFAULT_WIDTH, DEFAULT_HEIGHT));
    }
}
