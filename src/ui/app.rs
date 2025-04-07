use open;
use std::{collections::HashMap, path::PathBuf};

use egui::{
    CentralPanel, Color32, Context as EguiContext, Frame, RichText, ScrollArea, SidePanel,
    TopBottomPanel, Ui,
};
use flume::Receiver;

use crate::{
    context::{Context, ContextNotification},
    docs::DocsNotification,
    lsp::LspNotification,
    mcp::McpNotification,
    project::Project,
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

pub struct App {
    context: Context,
    receiver: Receiver<ContextNotification>,
    selected_project: Option<PathBuf>,
    logs: Vec<String>,
    events: HashMap<String, Vec<ContextNotification>>,
    selected_sidebar_tab: SidebarTab,
}

impl App {
    pub fn new(context: Context, receiver: Receiver<ContextNotification>) -> Self {
        Self {
            context,
            receiver,
            selected_project: None,
            logs: Vec::new(),
            events: HashMap::new(),
            selected_sidebar_tab: SidebarTab::Projects,
        }
    }

    fn handle_notifications(&mut self) {
        while let Ok(notification) = self.receiver.try_recv() {
            dbg!(&notification);
            let project_name = notification.project_name();
            self.events
                .entry(project_name)
                .or_default()
                .push(notification);
        }
    }

    fn draw_left_sidebar(&mut self, ui: &mut Ui, project_descriptions: &[ProjectDescription]) {
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

                let cell = ListCell::new(&project.name, is_selected, is_spinning);
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
                    tracing::info!("Adding project: {:?}", path_buf);

                    let context = self.context.clone();
                    tokio::spawn(async move {
                        if let Err(e) = context
                            .add_project(Project {
                                root: path_buf,
                                ignore_crates: vec![],
                            })
                            .await
                        {
                            tracing::error!("Failed to add project: {}", e);
                        } else {
                            tracing::info!("Project added successfully.");
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
                    self.logs.push(format!(
                        "Remove Project clicked for: {:?} (removal not implemented)",
                        selected_root
                    ));
                }
            }
        });
    }

    fn draw_info_tab(&mut self, ui: &mut Ui) {
        let (host, port) = self.context.address_information();
        let config_file = self.context.configuration_file();
        // TODO: Replace placeholders with actual data and functionality
        ui.label(format!("Address: {}", host));
        ui.label(format!("Port: {}", port));

        ui.add_space(10.0);

        ui.vertical_centered_justified(|ui| {
            if ui.button("Copy MCP JSON").clicked() {
                let config = self.context.mcp_configuration();
                ui.output_mut(|o| o.copied_text = config);
            }
            ui.small("Place this in your .cursor/mcp.json file");

            if ui.button("Open Conf").clicked() {
                open::that(shellexpand::tilde(&config_file).to_string());
            }
            if ui.button("Copy Conf Path").clicked() {
                let path = shellexpand::tilde(&config_file).to_string();
                ui.output_mut(|o| o.copied_text = path);
            }
            ui.small(&config_file);
            ui.small("To manually edit projects");
        });
    }

    fn draw_main_area(&mut self, ui: &mut Ui, project_descriptions: &[ProjectDescription]) {
        if let Some(selected_root) = &self.selected_project {
            if let Some(project) = project_descriptions
                .iter()
                .find(|p| p.root == *selected_root)
            {
                ui.vertical(|ui| {
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
                        ui.add_space(10.0);
                        if project.is_indexing_lsp {
                            ui.add(egui::Spinner::new());
                            ui.label("Indexing LSP...");
                        }
                        ui.add_space(10.0);
                        if project.is_indexing_docs {
                            ui.add(egui::Spinner::new());
                            ui.label("Indexing Docs...");
                        }
                    });

                    ui.separator();

                    ui.label("Events:");
                    ScrollArea::vertical().show(ui, |ui| {
                        if let Some(project_events) = self.events.get(&project.name) {
                            for event in project_events {
                                let event_str = match event {
                                    ContextNotification::Lsp(LspNotification::Indexing {
                                        ..
                                    }) => continue,
                                    ContextNotification::Docs(DocsNotification::Indexing {
                                        is_indexing,
                                        ..
                                    }) => {
                                        format!(
                                            "Docs Indexing: {}",
                                            if *is_indexing { "Started" } else { "Finished" }
                                        )
                                    }
                                    ContextNotification::Mcp(McpNotification::Request {
                                        content,
                                        ..
                                    }) => {
                                        format!("MCP Request: {:?}", content)
                                    }
                                    ContextNotification::Mcp(McpNotification::Response {
                                        content,
                                        ..
                                    }) => {
                                        format!("MCP Response: {:?}", content)
                                    }
                                    ContextNotification::ProjectAdded(project) => {
                                        format!("Project Added: {:?}", project)
                                    }
                                    ContextNotification::ProjectRemoved(project) => {
                                        format!("Project Removed: {:?}", project)
                                    }
                                };
                                ui.label(event_str);
                                ui.separator();
                            }
                        } else {
                            ui.label("No events for this project yet.");
                        }
                    });
                });
            } else {
                ui.label("Error: Selected project not found.");
                self.selected_project = None;
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label("Select a project from the left sidebar");
            });
        }
    }

    fn draw_bottom_bar(&mut self, ui: &mut Ui) {
        ui.label("Logs:");
        ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for log_entry in &self.logs {
                ui.label(log_entry);
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
        self.handle_notifications();
        let project_descriptions = self.context.project_descriptions();

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

        TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                self.draw_bottom_bar(ui);
            });

        CentralPanel::default().show(ctx, |ui| {
            self.draw_main_area(ui, &project_descriptions);
        });

        if !self.receiver.is_empty() {
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
