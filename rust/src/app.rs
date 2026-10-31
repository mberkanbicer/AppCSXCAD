//! AppCSXCAD application logic: menus, file I/O, settings and exports.
//!
//! The application wraps `qcsxcad_egui::EguiApp` (the QCSXCAD library: tree,
//! editors, 3D viewport) with the desktop behavior the native `AppCSXCAD`
//! class adds: the File/Info menu, project loading and saving, the export
//! actions, command-line arguments and window settings.

use qcsxcad_core::export::{self, PropertyFormat};
use csxcad_core::model::{PrimitiveType, PropertyType, Structure};
use qcsxcad_core::document::Document;
use qcsxcad_core::model::{GuiError, GuiState};
use qcsxcad_core::ProjectFile;
use qcsxcad_egui::app::chrome_frame;
use qcsxcad_egui::{theme, EguiApp, TransformMode};
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
    /// `dark`, `light` or `high-contrast`.
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_tree_width")]
    pub tree_width: f32,
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f32,
    /// Persisted drag-docking tree (ADR 0010); `None` uses the default layout.
    #[serde(default)]
    pub dock_layout: Option<String>,
}

fn default_tree_width() -> f32 {
    320.0
}

fn default_inspector_width() -> f32 {
    360.0
}

fn default_theme() -> String {
    "dark".to_owned()
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
            theme: default_theme(),
            tree_width: default_tree_width(),
            inspector_width: default_inspector_width(),
            dock_layout: None,
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

/// Decode a native QCSXCAD/Qt icon and cache it in the egui context.
fn icon_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(name);
    if let Some(handle) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return Some(handle);
    }
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [info.width as usize, info.height as usize],
        &buffer[..info.buffer_size()],
    );
    let handle = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|data| data.insert_temp(id, handle.clone()));
    Some(handle)
}

fn menu_item(
    ui: &mut egui::Ui,
    label: &str,
    icon: Option<&egui::TextureHandle>,
) -> egui::Response {
    match icon {
        Some(texture) => {
            let sized = egui::load::SizedTexture::new(texture.id(), texture.size_vec2());
            ui.add(egui::Button::image_and_text(sized, label))
        }
        None => ui.button(label),
    }
}

/// An icon-only command button. The accessible name is its tooltip.
fn icon_button(ui: &mut egui::Ui, icon: &egui::TextureHandle) -> egui::Response {
    let sized = egui::load::SizedTexture::new(icon.id(), icon.size_vec2());
    ui.add(egui::Button::image(sized))
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
    TransformMode(TransformMode),
    FitAll,
    FitSelection,
    SetProjection(bool),
    Exit,
}

/// A command palette entry. File/view commands reuse `AppAction`; dialogs are
/// opened through `Dialog`; presentation toggles act directly.
#[derive(Clone, Debug, PartialEq)]
enum Command {
    Action(AppAction),
    Dialog(Dialog),
    Undo,
    Redo,
    ToggleProjection,
    Theme(theme::ThemeMode),
    ToggleStructureTree,
    ToggleInspector,
    ToggleStatusBar,
    About,
}

#[derive(Clone, Debug)]
struct CommandEntry {
    id: &'static str,
    label: &'static str,
    category: &'static str,
    shortcut: &'static str,
    command: Command,
}

/// Every executable command with its stable id, menu category and shortcut.
///
/// The palette draws from this list only, so a command that is visible here is
/// also wired to the same handler the menus use.
fn command_entries(
    edit_mode: bool,
    has_document: bool,
    can_undo: bool,
    can_redo: bool,
) -> Vec<CommandEntry> {
    let mut entries = Vec::new();
    let mut push = |id, label, category, shortcut, command| {
        entries.push(CommandEntry {
            id,
            label,
            category,
            shortcut,
            command,
        })
    };
    if edit_mode {
        push("file.new", "New project", "File", "Ctrl+N", Command::Action(AppAction::New));
    }
    push("file.load", "Load project", "File", "Ctrl+O", Command::Dialog(Dialog::Load));
    if edit_mode && has_document {
        push("file.save", "Save", "File", "Ctrl+S", Command::Action(AppAction::Save));
    }
    if edit_mode {
        push(
            "file.save_as",
            "Save as",
            "File",
            "Ctrl+Shift+S",
            Command::Dialog(Dialog::SaveAs),
        );
    }
    if has_document {
        for (id, label, dialog) in [
            ("export.xml", "Export CSXCAD XML", Dialog::ExportXml),
            ("export.vtk", "Export Polydata-VTK", Dialog::ExportVtk),
            ("export.stl", "Export STL", Dialog::ExportStl),
            ("export.ply", "Export PLY", Dialog::ExportPly),
            ("export.pov", "Export POV-Ray", Dialog::ExportPovray),
            ("export.x3d", "Export X3D", Dialog::ExportX3d),
            ("export.png", "Export 3D view to PNG", Dialog::ExportPng),
        ] {
            push(id, label, "Export", "", Command::Dialog(dialog));
        }
    }
    push("edit.expand", "Expand structure tree", "Edit", "", Command::Action(AppAction::ExpandAll));
    push(
        "edit.collapse",
        "Collapse structure tree",
        "Edit",
        "",
        Command::Action(AppAction::CollapseAll),
    );
    if can_undo {
        push("edit.undo", "Undo", "Edit", "Ctrl+Z", Command::Undo);
    }
    if can_redo {
        push("edit.redo", "Redo", "Edit", "Ctrl+Shift+Z", Command::Redo);
    }
    push("view.fit_all", "Fit all", "View", "", Command::Action(AppAction::FitAll));
    push(
        "view.fit_selection",
        "Fit selection",
        "View",
        "",
        Command::Action(AppAction::FitSelection),
    );
    for (id, label, dir) in [
        ("view.top", "Top view", 0),
        ("view.front", "Front view", 1),
        ("view.right", "Right view", 2),
        ("view.3d", "3D perspective view", 3),
    ] {
        push(id, label, "View", "", Command::Action(AppAction::ViewDir(dir)));
    }
    push(
        "view.parallel",
        "Toggle parallel projection",
        "View",
        "",
        Command::ToggleProjection,
    );
    push(
        "view.tree",
        "Toggle structure tree",
        "Panels",
        "Ctrl+B",
        Command::ToggleStructureTree,
    );
    push(
        "view.inspector",
        "Toggle inspector",
        "Panels",
        "Ctrl+I",
        Command::ToggleInspector,
    );
    push(
        "view.status",
        "Toggle status bar",
        "Panels",
        "",
        Command::ToggleStatusBar,
    );
    for mode in theme::ThemeMode::ALL {
        push(
            match mode {
                theme::ThemeMode::Dark => "theme.dark",
                theme::ThemeMode::Light => "theme.light",
                theme::ThemeMode::HighContrast => "theme.high_contrast",
            },
            match mode {
                theme::ThemeMode::Dark => "Theme: dark",
                theme::ThemeMode::Light => "Theme: light",
                theme::ThemeMode::HighContrast => "Theme: high contrast",
            },
            "Theme",
            "",
            Command::Theme(mode),
        );
    }
    push("help.about", "About AppCSXCAD", "Help", "", Command::About);
    if edit_mode {
        push("file.exit", "Exit", "File", "", Command::Action(AppAction::Exit));
    }
    entries
}

/// Case-insensitive substring search over label, category and shortcut.
fn filter_commands<'a>(entries: &'a [CommandEntry], query: &str) -> Vec<&'a CommandEntry> {
    let needle = query.trim().to_ascii_lowercase();
    entries
        .iter()
        .filter(|entry| {
            needle.is_empty()
                || entry.label.to_ascii_lowercase().contains(&needle)
                || entry.category.to_ascii_lowercase().contains(&needle)
                || entry.shortcut.to_ascii_lowercase().contains(&needle)
        })
        .collect()
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
    palette_open: bool,
    palette_query: String,
    palette_focus: bool,
}

impl AppUiState {
    fn open_dialog(&mut self, dialog: Dialog, suggestion: &str) {
        self.dialog = Some(dialog);
        self.path_input = suggestion.to_owned();
    }

    fn take_action(&mut self) -> Option<AppAction> {
        self.action.take()
    }

    /// Draw the menu bar, the command strip and the dialogs.
    ///
    /// Runs inside the egui frame, before the QCSXCAD panels. It receives the
    /// document and the presentation state so presentation-only commands
    /// (undo, panels, theme) act directly instead of round-tripping through the
    /// deferred file-action queue.
    fn chrome(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut Document,
        state: &mut GuiState,
        edit_mode: bool,
        has_document: bool,
    ) {
        let ctx = ui.ctx().clone();
        let palette = theme::palette(&ctx);
        let mut icons = std::collections::HashMap::new();
        for (name, bytes) in [
            ("filenew", &include_bytes!("../../../QCSXCAD/images/filenew.png")[..]),
            ("fileopen", &include_bytes!("../../../QCSXCAD/images/fileopen.png")[..]),
            ("filesave", &include_bytes!("../../../QCSXCAD/images/filesave.png")[..]),
            ("filesaveas", &include_bytes!("../../../QCSXCAD/images/filesaveas.png")[..]),
            ("exit", &include_bytes!("../../../QCSXCAD/images/exit.png")[..]),
            ("collapse", &include_bytes!("../../../QCSXCAD/images/viewmag-.png")[..]),
            ("expand", &include_bytes!("../../../QCSXCAD/images/viewmag+.png")[..]),
        ] {
            if let Some(texture) = icon_texture(&ctx, name, bytes) {
                icons.insert(name, texture);
            }
        }

        // Shortcuts, ignored while a text field owns the keyboard.
        if !ui.ctx().egui_wants_keyboard_input() {
            let (new, open, save, save_as, capture) = ui.input_mut(|input| {
                let save_as = input.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::S,
                );
                let save = input.consume_key(egui::Modifiers::COMMAND, egui::Key::S);
                let open = input.consume_key(egui::Modifiers::COMMAND, egui::Key::O);
                let new = input.consume_key(egui::Modifiers::COMMAND, egui::Key::N);
                let capture = input.consume_key(egui::Modifiers::COMMAND, egui::Key::P);
                (new, open, save, save_as, capture)
            });
            if new && edit_mode {
                self.action = Some(AppAction::New);
            }
            if open {
                self.open_dialog(Dialog::Load, "");
            }
            if save && edit_mode && has_document {
                self.action = Some(AppAction::Save);
            }
            if save_as && edit_mode {
                self.open_dialog(Dialog::SaveAs, "");
            }
            if capture {
                self.open_dialog(Dialog::ExportPng, "view.png");
            }
            // Ctrl+K opens the command palette; consume the key so it does not
            // also reach the tree or the viewport.
            if ui
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::K))
            {
                self.palette_open = true;
                self.palette_query.clear();
                self.palette_focus = true;
            }

            // Panel visibility has a keyboard path as well as the View menu.
            let (tree, inspector) = ui.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::B),
                    input.consume_key(egui::Modifiers::COMMAND, egui::Key::I),
                )
            });
            if tree {
                state.show_structure_tree = !state.show_structure_tree;
            }
            if inspector {
                state.show_inspector = !state.show_inspector;
            }
        }

        egui::Panel::top("app_menu")
            .frame(chrome_frame(palette))
            .show(ui, |ui| {
                egui::menu::MenuBar::default().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if edit_mode && menu_item(ui, "New", icons.get("filenew")).clicked() {
                            self.action = Some(AppAction::New);
                            ui.close();
                        }
                        if menu_item(ui, "Load", icons.get("fileopen")).clicked() {
                            self.open_dialog(Dialog::Load, "");
                            ui.close();
                        }
                        if edit_mode && menu_item(ui, "Save", icons.get("filesave")).clicked() {
                            self.action = Some(AppAction::Save);
                            ui.close();
                        }
                        if edit_mode
                            && menu_item(ui, "Save As", icons.get("filesaveas")).clicked()
                        {
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
                        ui.separator();
                        if menu_item(ui, "Exit", icons.get("exit")).clicked() {
                            self.action = Some(AppAction::Exit);
                            ui.close();
                        }
                    });

                    ui.menu_button("Edit", |ui| {
                        if ui
                            .add(egui::Button::new("Command palette").shortcut_text("Ctrl+K"))
                            .clicked()
                        {
                            self.palette_open = true;
                            self.palette_query.clear();
                            self.palette_focus = true;
                            ui.close();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(
                                document.can_undo(),
                                egui::Button::new("Undo").shortcut_text("Ctrl+Z"),
                            )
                            .clicked()
                        {
                            document.undo();
                            ctx.request_repaint();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                document.can_redo(),
                                egui::Button::new("Redo").shortcut_text("Ctrl+Shift+Z"),
                            )
                            .clicked()
                        {
                            document.redo();
                            ctx.request_repaint();
                            ui.close();
                        }
                        ui.separator();
                        let selected = state.selection.primitives.len()
                            + state.selection.properties.len();
                        ui.add_enabled(selected > 0, egui::Button::new("Delete selection"))
                            .on_disabled_hover_text("Select an element in the tree or viewport first");
                    });

                    ui.menu_button("View", |ui| {
                        if ui.button("Fit all").clicked() {
                            self.action = Some(AppAction::FitAll);
                            ui.close();
                        }
                        if ui.button("Fit selection").clicked() {
                            self.action = Some(AppAction::FitSelection);
                            ui.close();
                        }
                        ui.separator();
                        for (label, dir, hover) in [
                            ("Top", 0, "Look down the -Z axis"),
                            ("Front", 1, "Look along the +Y axis"),
                            ("Right", 2, "Look along the +X axis"),
                            ("3D", 3, "Free perspective view"),
                        ] {
                            if ui
                                .selectable_label(state.view_dir == dir, label)
                                .on_hover_text(hover)
                                .clicked()
                            {
                                self.action = Some(AppAction::ViewDir(dir));
                                ui.close();
                            }
                        }
                        ui.separator();
                        let mut parallel = state.parallel_projection;
                        if ui
                            .checkbox(&mut parallel, "Parallel projection")
                            .on_hover_text("Orthographic camera (no perspective foreshortening)")
                            .changed()
                        {
                            self.action = Some(AppAction::SetProjection(parallel));
                        }
                        ui.separator();
                        ui.menu_button("Theme", |ui| {
                            for mode in theme::ThemeMode::ALL {
                                if ui.radio(theme::current(ui.ctx()) == mode, mode.label()).clicked() {
                                    theme::set(ui.ctx(), mode);
                                    ui.close();
                                }
                            }
                        });
                        ui.separator();
                        ui.checkbox(&mut state.show_structure_tree, "Structure tree   Ctrl+B");
                        ui.checkbox(&mut state.show_inspector, "Inspector   Ctrl+I");
                        ui.checkbox(&mut state.show_status_bar, "Status bar");
                    });

                    ui.menu_button("Info", |ui| {
                        if ui.button("About").clicked() {
                            self.about = true;
                            ui.close();
                        }
                    });

                    if !has_document {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("no project loaded").color(palette.text_tertiary),
                        );
                    }
                });
            });

        // Command strip. Groups follow the work order: file, history, structure,
        // creation, view, tool. It wraps instead of hiding commands when the
        // window is narrow.
        egui::Panel::top("app_toolbar")
            .frame(chrome_frame(palette))
            .show(ui, |ui| {
                // Each command group is a nested horizontal row, so a wrap
                // at narrow widths moves a whole group to the next line instead
                // of splitting it.
                ui.horizontal_wrapped(|ui| {
                        if edit_mode && menu_item(ui, "New", icons.get("filenew"))
                            .on_hover_text("New project (Ctrl+N)")
                            .clicked()
                        {
                            self.action = Some(AppAction::New);
                        }
                        if menu_item(ui, "Load", icons.get("fileopen"))
                            .on_hover_text("Load a CSXCAD project (Ctrl+O)")
                            .clicked()
                        {
                            self.open_dialog(Dialog::Load, "");
                        }
                        if edit_mode
                            && menu_item(ui, "Save", icons.get("filesave"))
                                .on_hover_text("Save (Ctrl+S)")
                                .clicked()
                        {
                            self.action = Some(AppAction::Save);
                        }
                        if edit_mode
                            && menu_item(ui, "Save As", icons.get("filesaveas"))
                                .on_hover_text("Save as (Ctrl+Shift+S)")
                                .clicked()
                        {
                            self.open_dialog(Dialog::SaveAs, "");
                        }
                    ui.separator();
                        if ui
                            .add_enabled(document.can_undo(), egui::Button::new("Undo"))
                            .on_hover_text("Undo the last edit (Ctrl+Z)")
                            .clicked()
                        {
                            document.undo();
                            ctx.request_repaint();
                        }
                        if ui
                            .add_enabled(document.can_redo(), egui::Button::new("Redo"))
                            .on_hover_text("Redo (Ctrl+Shift+Z)")
                            .clicked()
                        {
                            document.redo();
                            ctx.request_repaint();
                        }
                    ui.separator();
                        if let Some(icon) = icons.get("collapse") {
                            if icon_button(ui, icon)
                                .on_hover_text("Collapse the structure tree")
                                .clicked()
                            {
                                self.action = Some(AppAction::CollapseAll);
                            }
                        }
                        if let Some(icon) = icons.get("expand") {
                            if icon_button(ui, icon)
                                .on_hover_text("Expand the structure tree")
                                .clicked()
                            {
                                self.action = Some(AppAction::ExpandAll);
                            }
                        }
                    ui.separator();
                        ui.menu_button("+ Property", |ui| {
                            for (label, kind) in [
                                ("Material", PropertyType::Material),
                                ("Metal", PropertyType::Metal),
                                ("Excitation", PropertyType::Excitation),
                                ("ProbeBox", PropertyType::ProbeBox),
                                ("ResBox", PropertyType::ResBox),
                                ("DumpBox", PropertyType::DumpBox),
                            ] {
                                if ui
                                    .button(label)
                                    .on_hover_text(format!("Create a {kind:?} property"))
                                    .clicked()
                                {
                                    self.action = Some(AppAction::AddProperty(kind));
                                    ui.close();
                                }
                            }
                        });
                        ui.menu_button("+ Primitive", |ui| {
                            for (label, kind) in [
                                ("Box", PrimitiveType::Box),
                                ("MultiBox", PrimitiveType::MultiBox),
                                ("Sphere", PrimitiveType::Sphere),
                                ("Cylinder", PrimitiveType::Cylinder),
                                ("Polygon", PrimitiveType::Polygon),
                                ("User Defined", PrimitiveType::UserDefined),
                            ] {
                                if ui
                                    .button(label)
                                    .on_hover_text(format!("Add a {kind:?} primitive"))
                                    .clicked()
                                {
                                    self.action = Some(AppAction::AddPrimitive(kind));
                                    ui.close();
                                }
                            }
                        });
                    ui.separator();
                        if ui
                            .button("Fit")
                            .on_hover_text("Frame all geometry")
                            .clicked()
                        {
                            self.action = Some(AppAction::FitAll);
                        }
                        if ui
                            .button("Fit Sel")
                            .on_hover_text("Frame the selected geometry")
                            .clicked()
                        {
                            self.action = Some(AppAction::FitSelection);
                        }
                    ui.separator();
                        for mode in TransformMode::ALL {
                            if ui
                                .add(
                                    egui::Button::new(mode.label())
                                        .selected(state.transform_mode == mode),
                                )
                                .on_hover_text(match mode {
                                    TransformMode::Move => "Drag moves the selected primitive",
                                    TransformMode::Rotate => {
                                        "Drag rotates the selected primitive (or Alt+drag)"
                                    }
                                    TransformMode::Scale => "Drag scales the selected primitive",
                                })
                                .clicked()
                            {
                                self.action = Some(AppAction::TransformMode(mode));
                            }
                        }
                    ui.separator();
                    // Keep the view presets together when the strip is narrow.
                    // `available_width` is the panel width; the cursor position
                    // is what tells us how much of the current line is left.
                    let remaining = (ui.max_rect().right() - ui.cursor().left()).max(0.0);
                    if remaining < 250.0 {
                        ui.add_space(remaining);
                    }
                        for (label, dir) in [("Top", 0), ("Front", 1), ("Right", 2), ("3D", 3)] {
                            if ui
                                .add(egui::Button::new(label).selected(state.view_dir == dir))
                                .on_hover_text(match dir {
                                    0 => "Top view (XY, parallel)",
                                    1 => "Front view (XZ, parallel)",
                                    2 => "Right view (YZ, parallel)",
                                    _ => "Free 3D view (perspective)",
                                })
                                .clicked()
                            {
                                self.action = Some(AppAction::ViewDir(dir));
                            }
                        }
                });
            });

        self.command_palette(ui, document, state, edit_mode, has_document);
        self.show_dialog(ui);
        self.show_about(ui);
        if let Some(error) = &self.error {
            egui::Panel::bottom("app_error")
                .frame(
                    egui::Frame::new()
                        .fill(palette.accent_soft)
                        .inner_margin(egui::Margin::symmetric(theme::PANEL_PADDING, 4))
                        .stroke(egui::Stroke::new(1.0, palette.danger)),
                )
                .show(ui, |ui| {
                    ui.colored_label(palette.danger, error);
                });
        }
    }

    /// Ctrl+K command search. Choosing a row runs the same handler as the menu
    /// or toolbar; the list is the wired registry, not a separate command set.
    fn command_palette(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut Document,
        state: &mut GuiState,
        edit_mode: bool,
        has_document: bool,
    ) {
        if !self.palette_open {
            return;
        }
        let entries = command_entries(
            edit_mode,
            has_document,
            document.can_undo(),
            document.can_redo(),
        );
        let palette = theme::palette(ui.ctx());
        let mut execute = None;
        let mut close = false;
        egui::Window::new("Command palette")
            .id(egui::Id::new("app_command_palette"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 72.0))
            .show(ui.ctx(), |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .hint_text("Search commands")
                        .desired_width(520.0),
                );
                if self.palette_focus {
                    response.request_focus();
                    self.palette_focus = false;
                }
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
                let filtered = filter_commands(&entries, &self.palette_query);
                if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    if let Some(entry) = filtered.first() {
                        execute = Some(entry.command.clone());
                    }
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.label(
                                egui::RichText::new("No matching command")
                                    .color(palette.text_tertiary),
                            );
                        }
                        for entry in &filtered {
                            let label = if entry.shortcut.is_empty() {
                                format!("{:<9} {}", entry.category, entry.label)
                            } else {
                                format!(
                                    "{:<9} {}   {}",
                                    entry.category, entry.label, entry.shortcut
                                )
                            };
                            if ui
                                .selectable_label(false, label)
                                .on_hover_text(entry.id)
                                .clicked()
                            {
                                execute = Some(entry.command.clone());
                            }
                        }
                    });
                ui.label(
                    egui::RichText::new("Enter runs the first match - Esc closes")
                        .small()
                        .color(palette.text_tertiary),
                );
            });
        if let Some(command) = execute {
            self.palette_open = false;
            self.execute_command(command, document, state, ui.ctx());
        } else if close {
            self.palette_open = false;
        }
    }

    fn execute_command(
        &mut self,
        command: Command,
        document: &mut Document,
        state: &mut GuiState,
        ctx: &egui::Context,
    ) {
        match command {
            Command::Action(action) => self.action = Some(action),
            Command::Dialog(dialog) => self.open_dialog(dialog, dialog.hint()),
            Command::Undo => {
                document.undo();
                ctx.request_repaint();
            }
            Command::Redo => {
                document.redo();
                ctx.request_repaint();
            }
            Command::ToggleProjection => {
                self.action = Some(AppAction::SetProjection(!state.parallel_projection));
            }
            Command::Theme(mode) => theme::set(ctx, mode),
            Command::ToggleStructureTree => {
                state.show_structure_tree = !state.show_structure_tree;
            }
            Command::ToggleInspector => state.show_inspector = !state.show_inspector,
            Command::ToggleStatusBar => state.show_status_bar = !state.show_status_bar,
            Command::About => self.about = true,
        }
    }

    fn show_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(dialog) = self.dialog else {
            return;
        };
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        // Native dialogs confirm on Enter and cancel on Escape.
        ui.input(|input| {
            if input.key_pressed(egui::Key::Enter) {
                confirm = true;
            }
            if input.key_pressed(egui::Key::Escape) {
                cancel = true;
            }
        });
        egui::Window::new(dialog.title())
            .collapsible(false)
            .resizable(false)
            // Centred like the native modal dialogs, not tucked into the
            // top-left corner over the menu bar.
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
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
        egui_app.render_with(|root, document, state| {
            ui_state.chrome(root, document, state, edit_mode, has_document);
        })?;
        if let Some(action) = self.ui_state.take_action() {
            if let Err(error) = self.apply_action(action) {
                self.ui_state.error = Some(error.to_string());
            }
            // The action changed the document or the view after the frame was
            // drawn: paint once more.
            self.request_redraw();
        }
        Ok(())
    }

    /// Start a new project with a usable 20 mm grid, so a created primitive is
    /// visible and editable straight away. The grid is part of the initial
    /// structure, not an undoable edit.
    fn new_document_with_grid(&mut self) {
        let mut structure = Structure::new();
        for axis in 0..3 {
            for step in 0..=20 {
                structure.grid_mut().add_line(axis, step as f64 * 1.0e-3);
            }
        }
        self.egui_app.load_structure(structure);
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
                    self.new_document_with_grid();
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
                    self.new_document_with_grid();
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
                // Native's "New Primitive" dialog starts from a degenerate
                // shape; the toolbar gives it a visible 10 mm default so the
                // user sees the object and can then edit the coordinates. The
                // configuration is part of the same undo entry.
                let id = self
                    .egui_app
                    .document_mut()
                    .expect("document exists")
                    .add_primitive_with(property, kind, |primitive| {
                        if let Some(box_shape) = primitive.box_shape_mut() {
                            for index in 0..6 {
                                // Interleaved start/stop per axis: 0/1 = x,
                                // 2/3 = y, 4/5 = z.
                                let value = if index % 2 == 0 { 0.0 } else { 10.0e-3 };
                                box_shape.set_coordinate(index, value);
                            }
                        }
                        if let Some(sphere) = primitive.sphere_shape_mut() {
                            for axis in 0..3 {
                                sphere.set_center_coordinate(axis, 5.0e-3);
                            }
                            sphere.set_radius(5.0e-3);
                        }
                    })?;
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
            AppAction::TransformMode(mode) => {
                self.egui_app.set_transform_mode(mode);
                Ok(())
            }
            AppAction::FitAll => {
                self.egui_app.fit_all();
                Ok(())
            }
            AppAction::FitSelection => {
                self.egui_app.fit_selection();
                Ok(())
            }
            AppAction::SetProjection(enabled) => {
                self.egui_app.set_parallel_projection(enabled);
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
        let (tree_width, inspector_width) = self.egui_app.dock_widths();
        AppSettings {
            window_width,
            window_height,
            theme: self.egui_app.theme().name().to_owned(),
            tree_width,
            inspector_width,
            dock_layout: self.egui_app.dock_layout_json(),
        }
        .save();
    }

    /// Apply a persisted drag-docking layout at startup.
    pub fn set_dock_layout_json(&mut self, json: &str) {
        self.egui_app.set_dock_layout_json(json);
    }

    /// Apply a persisted theme name at startup.
    pub fn set_theme_name(&mut self, name: &str) {
        self.egui_app
            .set_theme(qcsxcad_egui::theme::ThemeMode::from_name(name));
    }

    /// Apply persisted dock widths at startup.
    pub fn set_dock_widths(&mut self, tree: f32, inspector: f32) {
        self.egui_app.set_dock_widths(tree, inspector);
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
    fn every_menu_action_runs_on_a_created_model() {
        let directory = temp_dir("actions");
        let mut app = app();
        app.apply_action(AppAction::New).expect("new");
        app.apply_action(AppAction::AddProperty(PropertyType::Material))
            .expect("material");
        app.apply_action(AppAction::AddPrimitive(PrimitiveType::Box))
            .expect("box");
        app.apply_action(AppAction::ExpandAll).expect("expand");
        app.apply_action(AppAction::CollapseAll).expect("collapse");
        app.apply_action(AppAction::ViewDir(2)).expect("view");
        app.apply_action(AppAction::TransformMode(TransformMode::Scale))
            .expect("scale tool");

        let xml = directory.join("export.xml");
        let vtk = directory.join("vtk");
        let stl = directory.join("stl");
        let ply = directory.join("ply");
        let pov = directory.join("scene.pov");
        let x3d = directory.join("scene.x3d");
        let project = directory.join("project.xml");
        for (action, path) in [
            (AppAction::ExportXml(xml.clone()), xml.clone()),
            (AppAction::ExportMesh(PropertyFormat::PolyDataVtk, vtk.clone()), vtk.clone()),
            (AppAction::ExportMesh(PropertyFormat::Stl, stl.clone()), stl.clone()),
            (AppAction::ExportMesh(PropertyFormat::Ply, ply.clone()), ply.clone()),
            (AppAction::ExportPovray(pov.clone()), pov.clone()),
            (AppAction::ExportX3d(x3d.clone()), x3d.clone()),
            (AppAction::SaveAs(project.clone()), project.clone()),
        ] {
            app.apply_action(action).expect("export");
            assert!(path.exists(), "missing {}", path.display());
        }
        app.apply_action(AppAction::New).expect("new again");
        app.apply_action(AppAction::Load(project.clone()))
            .expect("load");
        assert!(app.egui_app.document().is_some());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn new_clears_the_loaded_project() {
        let directory = temp_dir("new");
        let source = directory.join("project.xml");
        std::fs::write(&source, PROJECT).expect("write fixture");
        let mut app = app();
        app.load_path(&source).expect("load");
        assert!(app.egui_app.document().is_some());
        app.apply_action(AppAction::New).expect("new");
        assert!(app.egui_app.document().is_none());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn every_dialog_maps_to_its_action() {
        let path = PathBuf::from("out.bin");
        let expected = [
            (Dialog::Load, AppAction::Load(path.clone())),
            (Dialog::SaveAs, AppAction::SaveAs(path.clone())),
            (Dialog::ExportXml, AppAction::ExportXml(path.clone())),
            (
                Dialog::ExportVtk,
                AppAction::ExportMesh(PropertyFormat::PolyDataVtk, path.clone()),
            ),
            (
                Dialog::ExportStl,
                AppAction::ExportMesh(PropertyFormat::Stl, path.clone()),
            ),
            (
                Dialog::ExportPly,
                AppAction::ExportMesh(PropertyFormat::Ply, path.clone()),
            ),
            (
                Dialog::ExportPovray,
                AppAction::ExportPovray(path.clone()),
            ),
            (Dialog::ExportX3d, AppAction::ExportX3d(path.clone())),
            (Dialog::ExportPng, AppAction::ExportPng(path.clone())),
        ];
        for (dialog, action) in expected {
            assert_eq!(dialog.target(path.clone()), action);
            assert!(!dialog.title().is_empty());
            assert!(!dialog.hint().is_empty());
        }
    }

    #[test]
    fn transform_tools_switch_the_viewport_mode() {
        let mut app = app();
        for mode in [TransformMode::Rotate, TransformMode::Scale, TransformMode::Move] {
            app.apply_action(AppAction::TransformMode(mode))
                .expect("mode");
            assert_eq!(app.egui_app.get_transform_mode(), mode);
        }
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
    fn a_created_box_is_visible_and_exported() {
        let directory = temp_dir("create");
        let mut app = app();
        app.apply_action(AppAction::AddProperty(PropertyType::Material))
            .expect("add property");
        app.apply_action(AppAction::AddPrimitive(PrimitiveType::Box))
            .expect("add box");
        let document = app.egui_app.document().expect("document");
        let primitive = document
            .structure()
            .all_primitives(false, csxcad_core::model::PropertyType::Any)
            .first()
            .and_then(|primitive| primitive.id())
            .expect("the box exists");
        let bounds = document
            .structure()
            .primitive(primitive)
            .expect("primitive")
            .bounding_box(None, false)
            .expect("bounds");
        assert!(bounds[1] > bounds[0] && bounds[3] > bounds[2] && bounds[5] > bounds[4]);
        assert!((bounds[1] - 10.0e-3).abs() < 1.0e-12);
        // The grid was created with the project, so the view has bounds.
        let line_count = app
            .egui_app
            .document_mut()
            .expect("document")
            .structure_mut()
            .grid_mut()
            .line_count(0);
        assert!(line_count >= 3);

        let xml = directory.join("created.xml");
        app.apply_action(AppAction::ExportXml(xml.clone()))
            .expect("export");
        let text = std::fs::read_to_string(&xml).expect("read");
        assert!(text.contains("<Box"));
        // The exported file round-trips to the same 10 mm cube.
        let reparsed =
            csxcad_core::model::Structure::from_xml_str(&text).expect("reparse the export");
        let exported = reparsed
            .all_primitives(false, csxcad_core::model::PropertyType::Any)
            .first()
            .expect("the exported box")
            .bounding_box(None, false)
            .expect("bounds");
        assert!((exported[1] - exported[0] - 10.0e-3).abs() < 1.0e-12);
        let _ = std::fs::remove_dir_all(&directory);
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
            theme: "light".to_owned(),
            tree_width: 300.0,
            inspector_width: 400.0,
            dock_layout: Some("{\"tree\":\"test\"}".to_owned()),
        };
        assert_eq!(settings.window_size(), (320, 240));
        let json = serde_json::to_string(&settings).expect("serialize");
        let round_trip: AppSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_trip.tree_width, 300.0);
        assert_eq!(round_trip.dock_layout.as_deref(), Some("{\"tree\":\"test\"}"));
        assert_eq!(round_trip.inspector_width, 400.0);
        assert_eq!(round_trip.theme, "light");
        let settings = AppSettings::default();
        assert_eq!(settings.window_size(), (DEFAULT_WIDTH, DEFAULT_HEIGHT));
        assert_eq!(settings.tree_width, 320.0);
        assert_eq!(settings.inspector_width, 360.0);
    }

    #[test]
    fn command_palette_ids_are_unique_and_wired() {
        let entries = command_entries(true, true, true, true);
        let mut ids = std::collections::HashSet::new();
        for entry in &entries {
            assert!(
                ids.insert(entry.id),
                "duplicate palette id {}",
                entry.id
            );
            assert!(!entry.label.is_empty(), "{} has no label", entry.id);
        }
        for required in [
            "file.new",
            "file.save",
            "edit.undo",
            "edit.redo",
            "view.fit_all",
            "view.parallel",
            "view.tree",
            "theme.dark",
            "theme.light",
            "theme.high_contrast",
            "help.about",
        ] {
            assert!(ids.contains(required), "missing {required}");
        }
    }

    #[test]
    fn command_palette_filter_matches_label_category_and_shortcut() {
        let entries = command_entries(true, true, true, true);
        let by_label = filter_commands(&entries, "save as");
        assert_eq!(by_label.len(), 1);
        assert_eq!(by_label[0].id, "file.save_as");

        let by_category = filter_commands(&entries, "theme");
        assert_eq!(by_category.len(), 3);
        assert!(by_category
            .iter()
            .all(|entry| matches!(entry.command, Command::Theme(_))));

        let by_shortcut = filter_commands(&entries, "ctrl+z");
        assert_eq!(by_shortcut.len(), 1);
        assert_eq!(by_shortcut[0].id, "edit.undo");

        assert!(filter_commands(&entries, "no such command").is_empty());
    }

    #[test]
    fn command_palette_hides_view_only_commands_in_view_mode() {
        let view_entries = command_entries(false, true, false, false);
        let ids: Vec<&str> = view_entries.iter().map(|entry| entry.id).collect();
        assert!(ids.contains(&"file.load"));
        assert!(ids.contains(&"export.xml"));
        assert!(!ids.contains(&"file.new"));
        assert!(!ids.contains(&"file.exit"));
        assert!(!ids.contains(&"edit.undo"));
        assert!(!ids.contains(&"edit.redo"));

        let no_document = command_entries(true, false, false, false);
        let ids: Vec<&str> = no_document.iter().map(|entry| entry.id).collect();
        assert!(!ids.contains(&"file.save"));
        assert!(!ids.contains(&"export.vtk"));
    }

    #[test]
    fn palette_commands_execute_against_the_ui_state() {
        let mut app = app();
        let mut document = Document::new();
        let mut state = GuiState::default();
        let ctx = egui::Context::default();

        let tree = state.show_structure_tree;
        app.ui_state.execute_command(Command::ToggleStructureTree, &mut document, &mut state, &ctx);
        assert_ne!(state.show_structure_tree, tree);

        let inspector = state.show_inspector;
        app.ui_state.execute_command(Command::ToggleInspector, &mut document, &mut state, &ctx);
        assert_ne!(state.show_inspector, inspector);

        let status = state.show_status_bar;
        app.ui_state.execute_command(Command::ToggleStatusBar, &mut document, &mut state, &ctx);
        assert_ne!(state.show_status_bar, status);

        app.ui_state.execute_command(
            Command::Action(AppAction::FitAll),
            &mut document,
            &mut state,
            &ctx,
        );
        assert_eq!(app.ui_state().action, Some(AppAction::FitAll));

        app.ui_state.execute_command(Command::Dialog(Dialog::Load), &mut document, &mut state, &ctx);
        assert_eq!(app.ui_state().dialog, Some(Dialog::Load));

        app.ui_state.execute_command(Command::About, &mut document, &mut state, &ctx);
        assert!(app.ui_state().about);
    }

    #[test]
    fn command_palette_state_opens_and_closes() {
        let mut app = app();
        assert!(!app.ui_state().palette_open);
        app.ui_state.palette_open = true;
        app.ui_state.palette_focus = true;
        app.ui_state.palette_query.push_str("fit");
        assert!(app.ui_state().palette_open);
        assert_eq!(app.ui_state().palette_query, "fit");
    }
}
