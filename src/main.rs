#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use active_win_pos_rs::{get_active_window, ActiveWindow, WindowPosition};
use device_query::{DeviceQuery, DeviceState, Keycode, MouseState};
use eframe::egui::{self, Layout, Ui};
use egui_extras::{Column, TableBuilder};
use egui_plot::{BarChart, Plot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
struct Config {
    output_directory: Option<String>,
    processes_with_longer_tracking: HashSet<String>,
}

fn main() -> Result<(), eframe::Error> {
    let cfg = match confy::load("time_back", None) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("Cannot load configuration using default");
            Config::default()
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        run_and_return: true,
        ..Default::default()
    };

    let file_name = generate_file_name();
    let (data, sum_plot_data, avg_plot_data, median_plot_data) =
        if let Some(output_directory) = &cfg.output_directory {
            let output_file = output_directory.to_owned() + "/" + &file_name;
            let (sum_plot_data, avg_plot_data, median_plot_data) =
                collect_previous_data(output_directory, &file_name).unwrap_or_default();
            if Path::new(&output_file).exists() {
                match std::fs::File::open(output_file) {
                    Ok(f) => (
                        serde_json::from_reader(f).unwrap_or(HashMap::new()),
                        sum_plot_data,
                        avg_plot_data,
                        median_plot_data,
                    ),
                    Err(_) => (
                        HashMap::new(),
                        sum_plot_data,
                        avg_plot_data,
                        median_plot_data,
                    ),
                }
            } else {
                (
                    HashMap::new(),
                    sum_plot_data,
                    avg_plot_data,
                    median_plot_data,
                )
            }
        } else {
            (HashMap::new(), Vec::new(), Vec::new(), Vec::new())
        };

    let window_time = Arc::new(Mutex::new(data));
    let config = Arc::new(Mutex::new(cfg));
    {
        let window_time = window_time.clone();
        let config = config.clone();
        std::thread::spawn(move || {
            let mut last_input = Instant::now();
            let mut last_save = Instant::now();
            let device_state = DeviceState::new();
            let mouse: MouseState = device_state.get_mouse();

            let save_duration = Duration::from_secs(5);
            let check_duration = Duration::from_millis(50);
            let long_gap_between_input = Duration::from_secs(10 * 60);
            let small_gap_between_input = Duration::from_secs(5);
            let mut mouse_position = mouse.coords;
            loop {
                std::thread::sleep(check_duration);
                let mouse: MouseState = device_state.get_mouse();
                let temp_position = mouse.coords;

                if mouse_position != temp_position {
                    mouse_position = temp_position;
                    last_input = Instant::now();
                }

                let keys: Vec<Keycode> = device_state.get_keys();
                if !keys.is_empty() {
                    last_input = Instant::now();
                }

                let active_window = match get_active_window() {
                    Ok(active_window) => active_window,
                    Err(()) => ActiveWindow {
                        title: String::default(),
                        process_path: PathBuf::default(),
                        app_name: String::default(),
                        window_id: String::default(),
                        process_id: 0,
                        position: WindowPosition::default(),
                    },
                };
                if let Ok(mut window_time) = window_time.lock() {
                    let processes_with_longer_tracking = config
                        .lock()
                        .unwrap()
                        .processes_with_longer_tracking
                        .clone();
                    let gap_between_input =
                        if processes_with_longer_tracking.contains(&active_window.app_name) {
                            long_gap_between_input
                        } else {
                            small_gap_between_input
                        };
                    if last_input.elapsed() <= gap_between_input {
                        *window_time
                            .entry(active_window.app_name)
                            .or_insert(Duration::default()) += check_duration;
                    }
                };

                if last_save.elapsed() > save_duration {
                    last_save = Instant::now();
                    let data = window_time.lock().unwrap().clone();
                    let output_directory = if let Ok(config) = config.lock() {
                        config.output_directory.clone()
                    } else {
                        None
                    };

                    if let Some(output_directory) = output_directory {
                        match std::fs::File::create(output_directory.to_owned() + "/" + &file_name)
                        {
                            Ok(f) => {
                                if let Err(e) = serde_json::to_writer(f, &data) {
                                    eprintln!("Error exporting the data: {}", e);
                                }
                            }
                            Err(e) => eprintln!("Error creating the export file: {}", e),
                        }
                    }
                }
            }
        });
    }

    let close = Rc::new(RefCell::new(false));
    loop {
        let window_time = window_time.clone();
        let config = config.clone();
        let close_inner = close.clone();
        let sum_plot_data = sum_plot_data.clone();
        let avg_plot_data = avg_plot_data.clone();
        let median_plot_data = median_plot_data.clone();
        eframe::run_native(
            "Time back!",
            options.clone(),
            Box::new(move |_cc| {
                Box::new(TimeBack {
                    window_time,
                    config,
                    close: close_inner,
                    show_plot: false,
                    plot_type: PlotType::Avg,
                    sum_chart: sum_plot_data,
                    avg_chart: avg_plot_data,
                    median_chart: median_plot_data,
                    settings_open: false,
                })
            }),
        )?;
        if *close.borrow() {
            break;
        }
    }
    Ok(())
}

fn collect_previous_data(
    output_directory: &str,
    current_file: &str,
) -> Result<
    (
        Vec<egui_plot::Bar>,
        Vec<egui_plot::Bar>,
        Vec<egui_plot::Bar>,
    ),
    std::io::Error,
> {
    let current_file = output_directory.to_owned() + "/" + current_file;
    let mut result: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
    let mut file_count = 0;
    for entry in std::fs::read_dir(Path::new(&output_directory))? {
        let entry = entry?;
        let p_entry = entry.path();
        let s_entry = p_entry.to_str().unwrap_or_default();
        if current_file != s_entry {
            if let Ok(f) = std::fs::File::open(entry.path()) {
                let data: HashMap<String, Duration> =
                    serde_json::from_reader(f).unwrap_or_default();
                for (k, v) in data {
                    result.entry(k).or_default().push(v);
                }
                file_count += 1;
            }
        }
    }
    let result_collect = result
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            egui_plot::Bar::new(i as f64, v.iter().sum::<Duration>().as_secs_f64()).name(k)
        })
        .collect::<Vec<_>>();
    let result_avg = result
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            egui_plot::Bar::new(
                i as f64,
                v.iter().sum::<Duration>().as_secs_f64() / file_count as f64,
            )
            .name(k)
        })
        .collect::<Vec<_>>();
    let result_median = result
        .iter()
        .enumerate()
        .map(|(i, (k, v))| {
            egui_plot::Bar::new(
                i as f64,
                if file_count % 2 == 0 {
                    let middle = v.len() / 2;
                    if middle >= 1 {
                        (v[middle - 1] + v[middle]).as_secs_f64() / 2.
                    } else {
                        0.
                    }
                } else if !v.is_empty() {
                    v[v.len() / 2].as_secs_f64()
                } else {
                    0.
                },
            )
            .name(k)
        })
        .collect::<Vec<_>>();
    Ok((result_collect, result_avg, result_median))
}

fn generate_file_name() -> String {
    chrono::Local::now()
        .date_naive()
        .to_string()
        .replace('-', "")
}

#[derive(PartialEq)]
enum PlotType {
    Sum,
    Avg,
    Median,
}

struct TimeBack {
    window_time: Arc<Mutex<HashMap<String, Duration>>>,
    config: Arc<Mutex<Config>>,
    close: Rc<RefCell<bool>>,
    show_plot: bool,
    plot_type: PlotType,
    sum_chart: Vec<egui_plot::Bar>,
    avg_chart: Vec<egui_plot::Bar>,
    median_chart: Vec<egui_plot::Bar>,
    settings_open: bool,
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
                        if config.output_directory.is_some() && ui.button("Settings").clicked() {
                            self.settings_open = true;
                        }
                        if self.settings_open {
                            self.display_configuration(ctx, &mut config);
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
                if ui.button("Show plot").clicked() {
                    self.show_plot = !self.show_plot;
                }
                if self.show_plot {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.plot_type, PlotType::Sum, "Sum");
                        ui.radio_value(&mut self.plot_type, PlotType::Avg, "Avg");
                        ui.radio_value(&mut self.plot_type, PlotType::Median, "Median");
                    });
                    ui.add_space(5.);
                    Plot::new("Sum").show(ui, |plot_ui| {
                        plot_ui.bar_chart(BarChart::new(match self.plot_type {
                            PlotType::Sum => self.sum_chart.clone(),
                            PlotType::Avg => self.avg_chart.clone(),
                            PlotType::Median => self.median_chart.clone(),
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
