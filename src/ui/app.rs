use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use egui::{CentralPanel, Color32, Context as EguiContext, RichText, ScrollArea, SidePanel, Ui};
use flume::Receiver;

use crate::{
    context::{Context, ContextNotification},
    project::Project,
    lsp::{LspNotification, IndexingProgress},
};

#[derive(Clone, Debug)]
pub struct ProjectDescription {
    pub root: PathBuf,
    pub name: String,
    pub is_indexing_lsp: bool,
    pub is_indexing_docs: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum SidebarTab {
    Projects,
    Info,
}

#[derive(Clone, Debug)]
pub struct TimestampedEvent(DateTime<Utc>, ContextNotification);

impl PartialEq for TimestampedEvent {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

pub struct App {
    context: Context,
    receiver: Receiver<ContextNotification>,
    selected_project: Option<PathBuf>,
    logs: Vec<String>,
    events: HashMap<String, Vec<TimestampedEvent>>,
    selected_sidebar_tab: SidebarTab,
    selected_event: Option<TimestampedEvent>,
    project_descriptions: Vec<ProjectDescription>,
    indexing_progress: HashMap<PathBuf, IndexingProgress>,
}

impl App {
    pub fn new(
        context: Context,
        receiver: Receiver<ContextNotification>,
        project_descriptions: Vec<ProjectDescription>,
    ) -> Self {
        Self {
            context,
            receiver,
            selected_project: None,
            logs: Vec::new(),
            events: HashMap::new(),
            selected_sidebar_tab: SidebarTab::Projects,
            selected_event: None,
            project_descriptions,
            indexing_progress: HashMap::new(),
        }
    }

    fn handle_notifications(&mut self) -> bool {
        let mut has_new_events = false;
        while let Ok(notification) = self.receiver.try_recv() {
            // Order is important here. New projects came in
            if let ContextNotification::ProjectDescriptions(project_descriptions) = notification {
                self.project_descriptions = project_descriptions;
                has_new_events = true;
                continue;
            }

            // If its not a new project notification, request projects
            self.context.request_project_descriptions();

            // Handle detailed indexing progress notifications
            if let ContextNotification::Lsp(LspNotification::IndexingProgress(progress)) = &notification {
                has_new_events = true;
                tracing::debug!("Received detailed indexing progress: {:?}", progress);
                
                // Store the progress information
                self.indexing_progress.insert(progress.project.clone(), progress.clone());
                
                // Also add it to events for the event list
                let project_path = notification.notification_path();
                let Some(project) = find_root_project(&project_path, &self.project_descriptions) else {
                    tracing::error!("Project not found: {:?}", project_path);
                    continue;
                };
                let project_name = project.file_name().unwrap().to_string_lossy().to_string();
                let timestamped_event = TimestampedEvent(chrono::Utc::now(), notification);
                self.events
                    .entry(project_name)
                    .or_default()
                    .push(timestamped_event);
                
                continue;
            }
            
            // Filter out high-volume LSP notifications but allow indexing notifications through
            if let ContextNotification::Lsp(lsp) = &notification {
                // Let indexing notifications through to update the UI spinner
                if matches!(lsp, LspNotification::Indexing { .. }) {
                    has_new_events = true;
                    tracing::debug!("Received LSP indexing notification: {:?}", notification);
                    let project_path = notification.notification_path();
                    let Some(project) = find_root_project(&project_path, &self.project_descriptions) else {
                        tracing::error!("Project not found: {:?}", project_path);
                        continue;
                    };
                    let project_name = project.file_name().unwrap().to_string_lossy().to_string();
                    let timestamped_event = TimestampedEvent(chrono::Utc::now(), notification);
                    self.events
                        .entry(project_name)
                        .or_default()
                        .push(timestamped_event);
                } else {
                    // Filter out other high-volume LSP notifications
                    has_new_events = true;
                    continue;
                }
            } else {
                // Otherwise, we have a new event
                has_new_events = true;
                tracing::debug!("Received notification: {:?}", notification);
                let project_path = notification.notification_path();
                let Some(project) = find_root_project(&project_path, &self.project_descriptions) else {
                    tracing::error!("Project not found: {:?}", project_path);
                    continue;
                };
                let project_name = project.file_name().unwrap().to_string_lossy().to_string();
                let timestamped_event = TimestampedEvent(chrono::Utc::now(), notification);
                self.events
                    .entry(project_name)
                    .or_default()
                    .push(timestamped_event);
            }
        }
        has_new_events
    }

    fn draw_left_sidebar(&mut self, ui: &mut Ui, project_descriptions: &[ProjectDescription]) {
        ui.add_space(10.0);
        ui.columns(2, |columns| {
            columns[0].selectable_value(
                &mut self.selected_sidebar_tab,
                SidebarTab::Projects,
                "Projects",
            );
            columns[1].selectable_value(&mut self.selected_sidebar_tab, SidebarTab::Info, "Info");
        });

        match self.selected_sidebar_tab {
            SidebarTab::Projects => {
                self.draw_projects_tab(ui, project_descriptions);
            }
            SidebarTab::Info => {
                self.draw_info_tab(ui);
            }
        }
    }

    fn draw_projects_tab(&mut self, ui: &mut Ui, project_descriptions: &[ProjectDescription]) {
        ScrollArea::vertical().show(ui, |ui| {
            let selected_path = self.selected_project.clone();
            for project in project_descriptions {
                let is_spinning = project.is_indexing_lsp || project.is_indexing_docs;
                let is_selected = selected_path.as_ref() == Some(&project.root);

                // Get detailed progress information if available
                let status_text = if let Some(progress) = self.indexing_progress.get(&project.root) {
                    if progress.is_indexing {
                        // Format a detailed status message
                        let mut text = project.name.clone();
                        
                        // Add file count if available
                        if let Some(files) = progress.estimated_files {
                            text.push_str(&format!(" ({} files", files));
                            
                            // Add crate count if available
                            if let Some(crates) = progress.crate_count {
                                text.push_str(&format!(", {} crates", crates));
                            }
                            
                            text.push(')');
                        }
                        
                        // Add the progress message
                        if let Some(ref msg) = progress.status_message {
                            text.push_str(&format!("\n{}", msg));
                        }
                        
                        // Add elapsed time
                        text.push_str(&format!(" - {}", progress.elapsed_time()));
                        
                        text
                    } else {
                        project.name.clone()
                    }
                } else {
                    project.name.clone()
                };

                let cell = ListCell::new(&status_text, is_selected, is_spinning);
                let response = cell.show(ui);

                if response.clicked() {
                    self.selected_project = Some(project.root.clone());
                    ui.ctx().request_repaint();
                }
            }
        });

        ui.vertical_centered_justified(|ui| {
            if ui.button("Add Project").clicked() {
                if let Some(path_buf) = rfd::FileDialog::new().pick_folder() {
                    tracing::debug!("Adding project: {:?}", path_buf);

                    let context = self.context.clone();
                    
                    tokio::spawn(async move {
                        // Extra validation before creating Project
                        if !path_buf.exists() {
                            tracing::error!("Selected path doesn't exist: {:?}", path_buf);
                            return;
                        }
                        
                        // Try to create the .docs-cache directory before adding the project
                        // This helps verify we have write permissions to the folder
                        let cache_dir = path_buf.join(".docs-cache");
                        if !cache_dir.exists() {
                            if let Err(e) = std::fs::create_dir_all(&cache_dir) {
                                tracing::error!("Failed to create .docs-cache directory: {}", e);
                                tracing::error!("This may indicate permission issues with the selected folder.");
                                return;
                            }
                        }

                        // Create Project struct directly but with additional validation
                        let project = Project {
                            root: path_buf.clone(),
                            ignore_crates: vec![],
                        };

                        if let Err(e) = context.add_project(project).await {
                            tracing::error!("Failed to add project: {}", e);
                            
                            // More detailed Windows error messages
                            if cfg!(windows) {
                                if e.to_string().contains("find the file specified") {
                                    tracing::error!("Windows path error: Make sure the folder exists and has no special characters");
                                    tracing::error!("You selected: {:?}", path_buf);
                                    tracing::error!("Try selecting a folder with a simpler path and no special characters");
                                } else if e.to_string().contains("Failed to initialize Docs") {
                                    tracing::error!("Failed to initialize documentation system");
                                    tracing::error!("Check if you have write permissions to create the .docs-cache folder");
                                }
                            }
                        } else {
                            tracing::debug!("Project added successfully.");
                        }
                    });
                }
            }

            let remove_enabled = self.selected_project.is_some();
            if ui
                .add_enabled(remove_enabled, egui::Button::new("Remove Project"))
                .clicked()
            {
                if let Some(selected_root) = self.selected_project.take() {
                    let context = self.context.clone();
                    tokio::spawn(async move {
                        let _ = context.remove_project(&selected_root).await;
                    });
                }
            }
        });
    }

    fn draw_info_tab(&mut self, ui: &mut Ui) {
        let (host, port) = self.context.address_information();
        
        // Use current directory for config file path
        let current_dir = std::env::current_dir().unwrap_or_default();
        let config_file = self.context.configuration_file(&current_dir);
        
        ui.label(format!("Address: {}", host));
        ui.label(format!("Port: {}", port));

        ui.add_space(10.0);

        ui.vertical_centered_justified(|ui| {
            if ui.button("Copy MCP JSON").clicked() {
                let config = self.context.mcp_configuration();
                ui.ctx().copy_text(config);
            }
            ui.small("Place this in your .cursor/mcp.json file");

            if ui.button("Open Conf").clicked() {
                let path = std::path::Path::new(&config_file);
                
                // Create parent directory if it doesn't exist
                if let Some(parent) = path.parent() {
                    if !parent.exists() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            tracing::error!("Failed to create config directory: {}", e);
                        }
                    }
                }
                
                // Try to create an empty file if it doesn't exist
                if !path.exists() {
                    if let Err(e) = std::fs::write(path, "") {
                        tracing::error!("Failed to create empty config file: {}", e);
                    }
                }
                
                // Now try to open it
                if let Err(e) = open::that(&config_file) {
                    tracing::error!("Failed to open config file: {}", e);
                }
            }
            if ui.button("Copy Conf Path").clicked() {
                ui.ctx().copy_text(config_file.clone());
            }
            ui.small(&config_file);
            ui.small("To manually edit projects");
        });
    }

    fn draw_main_area(&mut self, ui: &mut Ui, project_descriptions: &[ProjectDescription]) {
        if let Some(selected_root) = &self.selected_project {
            let config_path = PathBuf::from(selected_root).join(".cursor").join("mcp.json");
            if let Some(project) = project_descriptions
                .iter()
                .find(|p| p.root == *selected_root)
            {
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("Update Docs Index").clicked() {
                            if let Some(ref selected_project) = self.selected_project {
                                let context = self.context.clone();
                                let selected_project = selected_project.clone();
                                tokio::spawn(async move {
                                    if let Err(e) =
                                        context.force_index_docs(&selected_project).await
                                    {
                                        tracing::error!("Failed to update docs index: {}", e);
                                    }
                                });
                            }
                            self.logs
                                .push(format!("Update Docs Index clicked for: {}", project.name));
                        }
                        if ui.button("Open Project").clicked() {
                            if let Err(e) = open::that(project.root.to_string_lossy().to_string()) {
                                tracing::error!("Failed to open project: {}", e);
                            }
                        }
                        if !config_path.exists()
                            && ui
                                .button("Install mcp.json")
                                .on_hover_text("Create a .cursor/mcp.json file in the project root")
                                .clicked()
                        {
                            let config = self.context.mcp_configuration();
                            if let Err(e) = create_mcp_configuration_file(&project.root, config) {
                                tracing::error!("Failed to create mcp.json: {}", e);
                            }
                        }
                        ui.add_space(10.0);
                        if project.is_indexing_lsp {
                            ui.horizontal(|ui| {
                                ui.add(egui::Spinner::new());
                                
                                // Show detailed progress information if available
                                if let Some(progress) = self.indexing_progress.get(&project.root) {
                                    // Show pause/resume button
                                    if progress.is_paused {
                                        if ui.button("▶ Resume").clicked() {
                                            let context = self.context.clone();
                                            let project_root = project.root.clone();
                                            tokio::spawn(async move {
                                                if let Err(e) = context.toggle_indexing_pause(&project_root, false).await {
                                                    tracing::error!("Failed to resume indexing: {}", e);
                                                }
                                            });
                                        }
                                    } else {
                                        if ui.button("⏸ Pause").clicked() {
                                            let context = self.context.clone();
                                            let project_root = project.root.clone();
                                            tokio::spawn(async move {
                                                if let Err(e) = context.toggle_indexing_pause(&project_root, true).await {
                                                    tracing::error!("Failed to pause indexing: {}", e);
                                                }
                                            });
                                        }
                                    }
                                    
                                    ui.label(progress.status_message());
                                } else {
                                    ui.label("Indexing LSP...");
                                }
                            });
                        }
                        ui.add_space(10.0);
                        if project.is_indexing_docs {
                            ui.add(egui::Spinner::new());
                            ui.label("Indexing Docs...");
                        }
                    });

                    // Allocate the remaining available space in the vertical layout
                    let remaining_space = ui.available_size_before_wrap();
                    ui.allocate_ui(remaining_space, |ui| {
                        // Show the dark frame within the allocated space
                        egui::Frame::dark_canvas(ui.style())
                            .fill(Color32::from_black_alpha(128))
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                // Make the ScrollArea fill the frame
                                ScrollArea::vertical()
                                    .auto_shrink([false, false]) // Don't shrink, fill space
                                    .show(ui, |ui| {
                                        if let Some(project_events) = self.events.get(&project.name)
                                        {
                                            let mut event_to_select = None;
                                            for event_tuple in project_events.iter().rev() {
                                                // Only filter out non-indexing LSP notifications
                                                if let ContextNotification::Lsp(lsp) = &event_tuple.1 {
                                                    if !matches!(lsp, 
                                                        crate::lsp::LspNotification::Indexing { .. } | 
                                                        crate::lsp::LspNotification::IndexingProgress(_)
                                                    ) {
                                                        continue;
                                                    }
                                                }
                                                let TimestampedEvent(timestamp, event) =
                                                    event_tuple;

                                                let timestamp_str =
                                                    timestamp.format("%H:%M:%S").to_string();

                                                let event_details_str = event.description();

                                                let full_event_str = format!(
                                                    "{} - {}",
                                                    timestamp_str, event_details_str
                                                );

                                                let is_selected = self.selected_event.as_ref()
                                                    == Some(event_tuple);

                                                let truncated_str = if full_event_str.len() > 120 {
                                                    format!("{}...", &full_event_str[..117])
                                                } else {
                                                    full_event_str
                                                };
                                                let response =
                                                    ui.selectable_label(is_selected, truncated_str);
                                                if response.clicked() {
                                                    event_to_select = Some(event_tuple.clone());
                                                }
                                            }
                                            if let Some(selected) = event_to_select {
                                                self.selected_event = Some(selected);
                                            }
                                        }
                                    });
                            });
                    });
                });
            } else {
                ui.label("Error: Selected project not found.");
                if self.selected_project.is_some() {
                    self.selected_event = None;
                }
                self.selected_project = None;
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("Select or add a project");
                ui.label("Added projects first need to be indexed for LSP and Docs before they can be used.");
            });
            if self.selected_event.is_some() {
                self.selected_event = None;
            }
        }
    }

    #[allow(dead_code)]
    fn draw_bottom_bar(&mut self, ui: &mut Ui) {
        ui.label("Logs:");
        ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for log_entry in &self.logs {
                ui.label(log_entry);
            }
        });
    }

    fn draw_right_sidebar(&mut self, ui: &mut Ui, event: TimestampedEvent) {
        ui.horizontal(|ui| {
            if ui.button("X").on_hover_text("Close").clicked() {
                self.selected_event = None;
            }
            if ui.button("Copy").on_hover_text("Copy").clicked() {
                ui.ctx().copy_text(format!("{:#?}", event.1));
            }
            ui.heading("Details");
        });
        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            ui.label(format!(
                "Timestamp: {}",
                event.0.format("%Y-%m-%d %H:%M:%S.%3f")
            ));
            ui.separator();
            ui.monospace(format!("{:#?}", event.1)); // Pretty-print the event
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
        let has_new_events = self.handle_notifications();
        let project_descriptions = self.project_descriptions.clone();

        let sidebar_frame = egui::Frame {
            fill: egui::Color32::from_rgb(32, 32, 32), // Darker background
            ..egui::Frame::side_top_panel(&ctx.style())
        };

        SidePanel::left("left_sidebar")
            .frame(sidebar_frame)
            .resizable(true)
            .default_width(200.0)
            .show(ctx, |ui| {
                self.draw_left_sidebar(ui, &project_descriptions);
            });

        // TopBottomPanel::bottom("bottom_panel")
        //     .resizable(true)
        //     .default_height(150.0)
        //     .show(ctx, |ui| {
        //         self.draw_bottom_bar(ui);
        //     });

        if let Some(event) = self.selected_event.clone() {
            SidePanel::right("right_sidebar")
                .resizable(true)
                .default_width(350.0) // You can adjust the default width
                .show(ctx, |ui| {
                    self.draw_right_sidebar(ui, event);
                });
        }

        CentralPanel::default().show(ctx, |ui| {
            self.draw_main_area(ui, &project_descriptions);
        });

        if has_new_events {
            ctx.request_repaint();
        }
    }
}

struct ListCell<'a> {
    text: &'a str,
    is_selected: bool,
    is_spinning: bool,
}

impl<'a> ListCell<'a> {
    /// Creates a new ListCell.
    fn new(text: &'a str, is_selected: bool, is_spinning: bool) -> Self {
        Self {
            text,
            is_selected,
            is_spinning,
        }
    }

    /// Draws the ListCell and returns the interaction response.
    fn show(self, ui: &mut Ui) -> egui::Response {
        // Calculate desired size (full width, standard height + padding)
        let desired_size = egui::vec2(
            ui.available_width(),
            ui.text_style_height(&egui::TextStyle::Body) + 2.0 * ui.style().spacing.item_spacing.y,
        );
        // Allocate space and sense clicks for the entire row
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        // Draw background highlight if selected or hovered
        let bg_fill = if self.is_selected {
            ui.style().visuals.selection.bg_fill
        } else if response.hovered() {
            ui.style().visuals.widgets.hovered.bg_fill
        } else {
            Color32::TRANSPARENT
        };

        if bg_fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(
                rect.expand(ui.style().spacing.item_spacing.x / 2.0),
                0, // No rounding
                bg_fill,
            );
        }

        // Draw the content (label and spinner) within the allocated rectangle
        let content_rect = rect.shrink(ui.style().spacing.item_spacing.x); // Add horizontal padding
        #[allow(deprecated)]
        let mut content_ui = ui.child_ui(
            content_rect,
            egui::Layout::left_to_right(egui::Align::Center),
            None,
        );

        content_ui.horizontal(|ui| {
            // Use a simple label, adjust text color if selected
            let text_color = if self.is_selected {
                ui.style().visuals.strong_text_color()
            } else {
                ui.style().visuals.text_color()
            };

            // Create a Label widget and set its sense to Hover only,
            // so it doesn't steal clicks from the parent response.
            let label = egui::Label::new(RichText::new(self.text).color(text_color))
                .selectable(false)
                .sense(egui::Sense::hover());
            ui.add(label);

            // Align spinner to the right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.is_spinning {
                    // Use the same text_color for the spinner for consistency
                    ui.add(egui::Spinner::new().color(text_color));
                }
            });
        });

        response
    }
}
fn find_root_project(mut path: &Path, projects: &[ProjectDescription]) -> Option<PathBuf> {
    if let Some(project) = projects.iter().find(|p| p.root == *path) {
        return Some(project.root.clone());
    }

    while let Some(parent) = path.parent() {
        path = parent;
        if let Some(project) = projects.iter().find(|p| p.root == *path) {
            return Some(project.root.clone());
        }
    }

    None
}

fn create_mcp_configuration_file(path: &Path, contents: String) -> anyhow::Result<()> {
    let cursor_dir = PathBuf::from(path).join(".cursor");
    std::fs::create_dir_all(&cursor_dir)?;
    let config_path = cursor_dir.join("mcp.json");
    std::fs::write(config_path, contents)?;
    Ok(())
}
