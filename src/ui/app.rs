use std::{collections::HashMap, path::PathBuf};

use egui::{CentralPanel, Context as EguiContext, ScrollArea, SidePanel, TopBottomPanel, Ui};
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

pub struct App {
    context: Context,
    receiver: Receiver<ContextNotification>,
    selected_project: Option<PathBuf>,
    logs: Vec<String>,
    events: HashMap<String, Vec<ContextNotification>>,
}

impl App {
    pub fn new(context: Context, receiver: Receiver<ContextNotification>) -> Self {
        Self {
            context,
            receiver,
            selected_project: None,
            logs: vec!["Log line 1".to_string(), "Log line 2".to_string()],
            events: HashMap::new(),
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
        ui.heading("Projects");

        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            let selected_path = self.selected_project.clone();
            for project in project_descriptions {
                let is_spinning = project.is_indexing_lsp || project.is_indexing_docs;
                let is_selected = selected_path.as_ref() == Some(&project.root);
                ui.horizontal(|ui| {
                    if ui.selectable_label(is_selected, &project.name).clicked() {
                        self.selected_project = Some(project.root.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if is_spinning {
                            ui.add(egui::Spinner::new());
                        }
                    });
                });
            }
        });

        ui.separator();

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

        SidePanel::left("left_sidebar")
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
