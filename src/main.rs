#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use active_win_pos_rs::{get_active_window, ActiveWindow, WindowPosition};
use app::TimeBack;
use dashmap::{DashMap, DashSet};
use device_query::{DeviceQuery, DeviceState, MouseState};
use eframe::egui;
use serde::{Deserialize, Serialize};
use utils::{calculate_avg, calculate_median, calculate_sum, generate_file_name};

mod app;
mod utils;

const INPUT_STATS_FILE: &str = "/input-stats";

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
struct Config {
    output_directory: Option<String>,
    processes_with_longer_tracking: DashSet<String>,
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
    let (windows_data, input_stats_data, graph_data) = if let Some(output_directory) =
        &cfg.output_directory
    {
        let output_file = output_directory.to_owned() + "/" + &file_name;
        let graph_data = collect_previous_data(output_directory, &file_name).unwrap_or_default();
        let windows_data = if Path::new(&output_file).exists() {
            match std::fs::File::open(output_file) {
                Ok(f) => serde_json::from_reader(f).unwrap_or(DashMap::new()),
                Err(_) => DashMap::new(),
            }
        } else {
            DashMap::new()
        };

        let input_stats_file = output_directory.to_owned() + INPUT_STATS_FILE;
        let input_stats_data = if Path::new(&input_stats_file).exists() {
            match std::fs::File::open(input_stats_file) {
                Ok(f) => serde_json::from_reader(f).unwrap_or(DashMap::new()),
                Err(_) => DashMap::new(),
            }
        } else {
            DashMap::new()
        };
        (windows_data, input_stats_data, graph_data)
    } else {
        (DashMap::new(), DashMap::new(), Vec::new())
    };

    let window_time = Arc::new(windows_data);
    let config = Arc::new(Mutex::new(cfg));
    let input_stats = Arc::new(input_stats_data);
    {
        let window_time = window_time.clone();
        let config = config.clone();
        let input_stats = input_stats.clone();
        // Collect the live data
        std::thread::spawn(move || {
            let mut last_input = Instant::now();
            let mut last_save = Instant::now();
            let device_state = DeviceState::new();
            let mouse: MouseState = device_state.get_mouse();

            let input_timer = Duration::from_millis(75);
            let save_timer = Duration::from_secs(5);
            let check_timer = Duration::from_millis(50);
            let long_gap_between_input = Duration::from_secs(10 * 60);
            let small_gap_between_input = Duration::from_secs(5);
            let mut mouse_position = mouse.coords;
            loop {
                std::thread::sleep(check_timer);
                let mouse: MouseState = device_state.get_mouse();
                let temp_position = mouse.coords;

                if last_input.elapsed() > input_timer {
                    for (i, button_pressed) in mouse.button_pressed.iter().enumerate() {
                        if *button_pressed {
                            *input_stats
                                .entry(format!("Mouse click: {}", i))
                                .or_insert(0) += 1;
                        }
                    }
                }
                if mouse_position != temp_position {
                    if last_input.elapsed() > input_timer {
                        *input_stats.entry("Mouse move".to_string()).or_insert(0) += 1;
                    }
                    mouse_position = temp_position;
                    last_input = Instant::now();
                }

                let keys = device_state.get_keys();
                if !keys.is_empty() {
                    if last_input.elapsed() > input_timer {
                        keys.into_iter()
                            .for_each(|k| *input_stats.entry(k.to_string()).or_insert(0) += 1);
                    }
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
                        .or_insert(Duration::default()) += check_timer;
                }

                if last_save.elapsed() > save_timer {
                    last_save = Instant::now();
                    let output_directory = if let Ok(config) = config.lock() {
                        config.output_directory.clone()
                    } else {
                        None
                    };

                    if let Some(output_directory) = output_directory {
                        match std::fs::File::create(output_directory.to_owned() + "/" + &file_name)
                        {
                            Ok(f) => {
                                if let Err(e) = serde_json::to_writer(f, &window_time) {
                                    eprintln!("Error exporting the data: {}", e);
                                }
                            }
                            Err(e) => eprintln!("Error creating the data export file: {}", e),
                        }
                        match std::fs::File::create(output_directory.to_owned() + INPUT_STATS_FILE)
                        {
                            Ok(f) => {
                                if let Err(e) = serde_json::to_writer(f, &input_stats) {
                                    eprintln!("Error exporting the input stats: {}", e);
                                }
                            }
                            Err(e) => {
                                eprintln!("Error creating the input stats export file: {}", e)
                            }
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
        let graph_data = graph_data.clone();
        let input_stats = input_stats.clone();
        eframe::run_native(
            "Time back!",
            options.clone(),
            Box::new(move |_cc| {
                Box::new(TimeBack {
                    window_time,
                    config,
                    close: close_inner,
                    show_plot: false,
                    plot_type: PlotType::Live,
                    graph_data,
                    settings_open: false,
                    input_stats_open: false,
                    input_stats,
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
) -> Result<Vec<Vec<egui_plot::Bar>>, std::io::Error> {
    let current_file = output_directory.to_owned() + "/" + current_file;
    let mut values: BTreeMap<String, Vec<Duration>> = BTreeMap::new();
    let mut file_count = 0;
    for entry in std::fs::read_dir(Path::new(&output_directory))? {
        let entry = entry?;
        let p_entry = entry.path();
        let s_entry = p_entry.to_str().unwrap_or_default();
        if current_file != s_entry {
            if let Ok(f) = std::fs::File::open(entry.path()) {
                let data: DashMap<String, Duration> =
                    serde_json::from_reader(f).unwrap_or_default();
                for (k, v) in data {
                    values.entry(k).or_default().push(v);
                }
                file_count += 1;
            }
        }
    }
    let mut result = Vec::with_capacity(PlotType::Live as usize);
    for _ in 0..PlotType::Live as usize {
        result.push(vec![]);
    }
    result[PlotType::Sum as usize] = calculate_sum(&values)
        .into_iter()
        .enumerate()
        .map(|(i, (k, v))| egui_plot::Bar::new(i as f64, v).name(k))
        .collect();
    result[PlotType::Avg as usize] = calculate_avg(&values, file_count)
        .into_iter()
        .enumerate()
        .map(|(i, (k, v))| egui_plot::Bar::new(i as f64, v).name(k))
        .collect();
    result[PlotType::Median as usize] = calculate_median(&values)
        .into_iter()
        .enumerate()
        .map(|(i, (k, v))| egui_plot::Bar::new(i as f64, v).name(k))
        .collect();
    Ok(result)
}

#[derive(PartialEq)]
enum PlotType {
    Sum = 0,
    Avg = 1,
    Median = 2,
    // Keep this as the last one as a count
    Live,
}
