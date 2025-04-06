use std::path::PathBuf;

use egui::{CentralPanel, Context as EguiContext, ScrollArea, SidePanel, TopBottomPanel, Ui, Vec2};
use flume::Receiver;

use crate::context::{Context, ContextNotification};

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
    project_descriptions: Vec<ProjectDescription>,
    logs: Vec<String>,
}

impl App {
    pub fn new(context: Context, receiver: Receiver<ContextNotification>) -> Self {
        let project_descriptions = context.project_descriptions();
        Self {
            context,
            receiver,
            selected_project: None,
            project_descriptions,
            logs: vec!["Log line 1".to_string(), "Log line 2".to_string()],
        }
    }

    fn handle_notifications(&mut self) {
        while let Ok(notification) = self.receiver.try_recv() {
            match notification {
                ContextNotification::Lsp(lsp_notification) => (),
                ContextNotification::Docs(docs_notification) => (),
                ContextNotification::Mcp(mcp_notification) => (),
                // ContextNotification::ProjectListChanged => {
                //     self.project_descriptions = self.context.project_descriptions();
                //     if let Some(selected) = &self.selected_project {
                //         if !self
                //             .project_descriptions
                //             .iter()
                //             .any(|p| p.root == *selected)
                //         {
                //             self.selected_project = None;
                //         }
                //     }
                // }
                // ContextNotification::Log(message) => {
                //     self.logs.push(message);
                //     if self.logs.len() > 1000 {
                //         self.logs.remove(0);
                //     }
                // }
                // ContextNotification::IndexingStatusChanged(root, is_lsp, is_docs) => {
                //     if let Some(proj) = self
                //         .project_descriptions
                //         .iter_mut()
                //         .find(|p| p.root == root)
                //     {
                //         proj.is_indexing_lsp = is_lsp;
                //         proj.is_indexing_docs = is_docs;
                //     }
                // }
            }
        }
    }

    fn draw_left_sidebar(&mut self, ui: &mut Ui) {
        ui.heading("Projects");

        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            let selected_path = self.selected_project.clone();
            for project in &self.project_descriptions {
                let is_selected = selected_path.as_ref() == Some(&project.root);
                if ui.selectable_label(is_selected, &project.name).clicked() {
                    self.selected_project = Some(project.root.clone());
                }
            }
        });

        ui.separator();

        ui.vertical_centered_justified(|ui| {
            if ui.button("Add Project").clicked() {
                self.logs
                    .push("Add Project clicked (modal not implemented)".to_string());
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

    fn draw_main_area(&mut self, ui: &mut Ui) {
        if let Some(selected_root) = &self.selected_project {
            if let Some(project) = self
                .project_descriptions
                .iter()
                .find(|p| p.root == *selected_root)
            {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Update Docs Index").clicked() {
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

                    ui.label("Scrollable content area (placeholder):");
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.add_space(ui.available_height());
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

        SidePanel::left("left_sidebar")
            .resizable(true)
            .default_width(200.0)
            .show(ctx, |ui| {
                self.draw_left_sidebar(ui);
            });

        TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .default_height(150.0)
            .show(ctx, |ui| {
                self.draw_bottom_bar(ui);
            });

        CentralPanel::default().show(ctx, |ui| {
            self.draw_main_area(ui);
        });

        if !self.receiver.is_empty() {
            ctx.request_repaint();
        }
    }
}
