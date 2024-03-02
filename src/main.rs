#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use active_win_pos_rs::{get_active_window, ActiveWindow, WindowPosition};
use device_query::{DeviceQuery, DeviceState, Keycode, MouseState};
use eframe::egui::{self, Layout};
use egui_extras::{Column, TableBuilder};
use serde::{Deserialize, Serialize};

const DEFAULT_OUTPUT_DIRECTORY: &str = "~/.time_back";

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    output_directory: String,
    configured: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_directory: DEFAULT_OUTPUT_DIRECTORY.to_owned(),
            configured: false,
        }
    }
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
    let output_file = cfg.output_directory.to_owned() + "/" + &file_name;
    let data: HashMap<String, Duration> = if Path::new(&output_file).exists() {
        match std::fs::File::open(output_file) {
            Ok(f) => serde_json::from_reader(f).unwrap_or(HashMap::new()),
            Err(_) => HashMap::new(),
        }
    } else {
        HashMap::new()
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
            let gap_between_input = Duration::from_secs(5);
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
                    if last_input.elapsed() <= gap_between_input {
                        *window_time
                            .entry(active_window.app_name)
                            .or_insert(Duration::default()) += check_duration;
                    }
                };

                if last_save.elapsed() > save_duration {
                    last_save = Instant::now();
                    let data = window_time.lock().unwrap().clone();
                    let output_directory = config.lock().unwrap().output_directory.clone();
                    let file_name = generate_file_name();
                    match std::fs::File::create(output_directory.to_owned() + "/" + &file_name) {
                        Ok(f) => {
                            if let Err(e) = serde_json::to_writer(f, &data) {
                                eprintln!("Error exporting the data: {}", e);
                            }
                        }
                        Err(e) => eprintln!("Error creating the export file: {}", e),
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
        eframe::run_native(
            "Time back!",
            options.clone(),
            Box::new(move |_cc| {
                Box::new(TimeBack {
                    window_time,
                    config,
                    close: close_inner,
                })
            }),
        )?;
        if *close.borrow() {
            break;
        }
    }
    Ok(())
}

fn generate_file_name() -> String {
    chrono::Local::now()
        .date_naive()
        .to_string()
        .replace('-', "")
}

struct TimeBack {
    window_time: Arc<Mutex<HashMap<String, Duration>>>,
    config: Arc<Mutex<Config>>,
    close: Rc<RefCell<bool>>,
}

impl Drop for TimeBack {
    fn drop(&mut self) {
        let data = self.window_time.lock().unwrap().clone();
        let output_directory = self.config.lock().unwrap().output_directory.clone();
        let file_name = generate_file_name();
        serde_json::to_writer(
            std::fs::File::create(output_directory.to_owned() + "/" + &file_name)
                .unwrap_or_else(|_| panic!("{} file not possible to create", file_name)),
            &data,
        )
        .unwrap();
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
                    if ui.button("Close").clicked() {
                        *self.close.borrow_mut() = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });

            if let Ok(mut config) = self.config.lock() {
                if config.configured {
                    let table_height = 20.;
                    let table = TableBuilder::new(ui)
                        .striped(true)
                        .resizable(false)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(Column::auto())
                        .column(Column::remainder())
                        .min_scrolled_height(0.0);
                    if let Ok(map) = self.window_time.lock() {
                        table.body(|mut body| {
                            let mut overall = Duration::new(0, 0);
                            for (n, d) in map.iter() {
                                body.row(table_height, |mut row| {
                                    row.col(|ui| {
                                        ui.label(n);
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
                } else {
                    if ui.button("Select output directory").clicked() {
                        config.output_directory = match tinyfiledialogs::select_folder_dialog(
                            "Select output directory",
                            "",
                        ) {
                            Some(result) => result,
                            None => DEFAULT_OUTPUT_DIRECTORY.to_string(),
                        };
                    }
                    ui.label(format!(
                        "Current output directory: {}",
                        config.output_directory
                    ));
                    if ui.button("Accept").clicked() {
                        config.configured = true;
                        match confy::store("time_back", None, &*config) {
                            Ok(_) => {}
                            Err(_) => {
                                config.configured = false;
                                ui.label("Error saving the configuration");
                            }
                        }
                    }
                }
            }
        });
        ctx.request_repaint();
    }
}
