use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use eframe::egui::{self, Layout, Ui};
use egui_extras::{Column, TableBuilder};
use egui_plot::{BarChart, Plot};

use hashbrown::HashMap;

use crate::{utils::generate_file_name, Config, PlotType};

pub enum InputType {}

pub struct TimeBack {
    pub window_time: Arc<Mutex<HashMap<String, Duration>>>,
    pub config: Arc<Mutex<Config>>,
    pub close: Rc<RefCell<bool>>,
    pub show_plot: bool,
    pub plot_type: PlotType,
    pub graph_data: Vec<Vec<egui_plot::Bar>>,
    pub settings_open: bool,
    pub input_stats_open: bool,
    pub input_stats: HashMap<String, u32>,
}

impl Drop for TimeBack {
    fn drop(&mut self) {
        let data = self.window_time.lock().unwrap().clone();
        let output_directory = self
            .config
            .lock()
            .map(|config| config.output_directory.clone())
            .unwrap();
        if let Some(output_directory) = output_directory {
            let file_name = generate_file_name();
            serde_json::to_writer(
                std::fs::File::create(output_directory.to_owned() + "/" + &file_name)
                    .unwrap_or_else(|_| panic!("{} file not possible to create", file_name)),
                &data,
            )
            .unwrap();
        }
    }
}

impl eframe::App for TimeBack {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(ppp) = ctx.native_pixels_per_point() {
            ctx.set_pixels_per_point(ppp);
        } else {
            ctx.set_pixels_per_point(2.);
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Time back!");
                ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                    let config = self.config.lock().map(|config| (*config).clone()).ok();
                    if let Some(mut config) = config {
                        if config.output_directory.is_some() {
                            if ui.button("Settings").clicked() {
                                self.settings_open = true;
                            }
                            if ui.button("Input stats").clicked() {
                                self.input_stats_open = true;
                            }
                        }
                        if self.settings_open {
                            self.display_configuration(ctx, &mut config);
                        }
                        if self.input_stats_open {
                            self.display_input_stats(ctx, &mut config);
                        }
                    }
                    if ui.button("Close").clicked() {
                        *self.close.borrow_mut() = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });

            let configured = if let Ok(mut config) = self.config.lock() {
                if config.output_directory.is_none() {
                    display_initial_configuration(ui, &mut config);
                    false
                } else {
                    true
                }
            } else {
                false
            };
            if configured {
                self.display_main_ui(ui);
            }
        });
        ctx.request_repaint();
    }
}

impl TimeBack {
    fn display_main_ui(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                let table_height = 20.;
                let table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(false)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto())
                    .column(Column::initial(100.))
                    .min_scrolled_height(500.0);
                if let Ok(map) = self.window_time.lock() {
                    if let Ok(mut config) = self.config.lock() {
                        table.body(|mut body| {
                            let mut overall = Duration::new(0, 0);
                            for (n, d) in map.iter() {
                                let mut checked = config.processes_with_longer_tracking.contains(n);
                                body.row(table_height, |mut row| {
                                    row.col(|ui| {
                                        if ui.checkbox(&mut checked, n).clicked() {
                                            if checked {
                                                config
                                                    .processes_with_longer_tracking
                                                    .insert(n.to_string());
                                            } else {
                                                config.processes_with_longer_tracking.remove(n);
                                            }
                                            match confy::store("time_back", None, &*config) {
                                                Ok(_) => {}
                                                Err(_) => {
                                                    ui.label("Error saving the configuration");
                                                }
                                            }
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(humantime::Duration::from(*d).to_string());
                                    });
                                    overall += *d;
                                })
                            }
                            body.row(table_height, |mut row| {
                                row.col(|_ui| {});
                                row.col(|_ui| {});
                            });
                            body.row(table_height, |mut row| {
                                row.col(|ui| {
                                    ui.label("Overall");
                                });
                                row.col(|ui| {
                                    ui.label(humantime::Duration::from(overall).to_string());
                                });
                            })
                        });
                    }
                }
            });
            ui.vertical(|ui| {
                if ui.button("Show graph").clicked() {
                    self.show_plot = !self.show_plot;
                }
                if self.show_plot {
                    ui.label("Only live graph includes today data");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.plot_type, PlotType::Live, "Live");
                        ui.radio_value(&mut self.plot_type, PlotType::Sum, "Sum");
                        ui.radio_value(&mut self.plot_type, PlotType::Avg, "Avg");
                        ui.radio_value(&mut self.plot_type, PlotType::Median, "Median");
                    });
                    ui.add_space(5.);
                    Plot::new("Sum").show(ui, |plot_ui| {
                        plot_ui.bar_chart(BarChart::new(match self.plot_type {
                            PlotType::Sum => self.graph_data[PlotType::Sum as usize].clone(),
                            PlotType::Avg => self.graph_data[PlotType::Avg as usize].clone(),
                            PlotType::Median => self.graph_data[PlotType::Median as usize].clone(),
                            PlotType::Live => {
                                if let Ok(window_time) = self.window_time.lock() {
                                    window_time
                                        .iter()
                                        .enumerate()
                                        .map(|(i, (k, v))| {
                                            egui_plot::Bar::new(i as f64, v.as_secs_f64()).name(k)
                                        })
                                        .collect()
                                } else {
                                    vec![]
                                }
                            }
                        }));
                    });
                }
            });
        });
    }

    fn display_configuration(&mut self, ctx: &egui::Context, config: &mut Config) {
        egui::Window::new("Settings")
            .open(&mut self.settings_open)
            .resizable(false)
            .show(ctx, |ui| {
                if ui.button("Select output directory").clicked() {
                    config.output_directory =
                        tinyfiledialogs::select_folder_dialog("Select output directory", "");
                }
                ui.label(format!(
                    "Current output directory: {:?}",
                    config.output_directory
                ));
                ui.separator();
                ui.heading("Long tracking processes");
                ui.horizontal(|ui| {
                    for p in config.processes_with_longer_tracking.iter() {
                        ui.label(p);
                        ui.end_row();
                    }
                });
                ui.separator();
                if ui.button("Accept").clicked() {
                    match confy::store("time_back", None, &*config) {
                        Ok(_) => {}
                        Err(_) => {
                            ui.label("Error saving the configuration");
                        }
                    }
                }
            });
    }

    fn display_input_stats(&mut self, ctx: &egui::Context, config: &mut Config) {
        egui::Window::new("Input stats")
            .open(&mut self.input_stats_open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    let table_height = 20.;
                    let table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(false)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto())
                        .column(Column::initial(100.))
                        .min_scrolled_height(500.0);
                    if let Ok(mut config) = self.config.lock() {
                        table.body(|mut body| {
                            for (n, c) in self.input_stats.iter() {
                                let mut checked = config.processes_with_longer_tracking.contains(n);
                                body.row(table_height, |mut row| {
                                    row.col(|ui| {
                                        if ui.checkbox(&mut checked, n).clicked() {
                                            if checked {
                                                config
                                                    .processes_with_longer_tracking
                                                    .insert(n.to_string());
                                            } else {
                                                config.processes_with_longer_tracking.remove(n);
                                            }
                                            match confy::store("time_back", None, &*config) {
                                                Ok(_) => {}
                                                Err(_) => {
                                                    ui.label("Error saving the configuration");
                                                }
                                            }
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(c.to_string());
                                    });
                                })
                            }
                            body.row(table_height, |mut row| {
                                row.col(|_ui| {});
                                row.col(|_ui| {});
                            });
                        });
                    }
                });
            });
    }
}

fn display_initial_configuration(ui: &mut Ui, config: &mut Config) {
    if ui.button("Select output directory").clicked() {
        config.output_directory =
            tinyfiledialogs::select_folder_dialog("Select output directory", "");
    }
    ui.label(format!(
        "Current output directory: {:?}",
        config.output_directory
    ));
    if ui.button("Accept").clicked() {
        match confy::store("time_back", None, &*config) {
            Ok(_) => {}
            Err(_) => {
                ui.label("Error saving the configuration");
            }
        }
    }
}
