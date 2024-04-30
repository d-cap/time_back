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
use device_query::{DeviceQuery, DeviceState, Keycode, MouseState};
use eframe::egui::{self};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use utils::{calculate_avg, calculate_median, calculate_sum, generate_file_name};

mod app;
mod utils;

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
    let (windows_data, input_stats_data, graph_data) = if let Some(output_directory) =
        &cfg.output_directory
    {
        let output_file = output_directory.to_owned() + "/" + &file_name;
        let graph_data = collect_previous_data(output_directory, &file_name).unwrap_or_default();
        let windows_data = if Path::new(&output_file).exists() {
            match std::fs::File::open(output_file) {
                Ok(f) => serde_json::from_reader(f).unwrap_or(HashMap::new()),
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };

        let input_stats_file = output_directory.to_owned() + "/input-stats";
        let input_stats_data = if Path::new(&input_stats_file).exists() {
            match std::fs::File::open(input_stats_file) {
                Ok(f) => serde_json::from_reader(f).unwrap_or(HashMap::new()),
                Err(_) => HashMap::new(),
            }
        } else {
            HashMap::new()
        };
        (windows_data, input_stats_data, graph_data)
    } else {
        (HashMap::new(), HashMap::new(), Vec::new())
    };

    let window_time = Arc::new(Mutex::new(windows_data));
    let config = Arc::new(Mutex::new(cfg));
    {
        let window_time = window_time.clone();
        let config = config.clone();
        // Collect the live data
        std::thread::spawn(move || {
            let mut last_input = Instant::now();
            let mut last_save = Instant::now();
            let device_state = DeviceState::new();
            let mouse: MouseState = device_state.get_mouse();

            let save_timer = Duration::from_secs(5);
            let check_timer = Duration::from_millis(50);
            let long_gap_between_input = Duration::from_secs(10 * 60);
            let small_gap_between_input = Duration::from_secs(5);
            let mut mouse_position = mouse.coords;
            loop {
                std::thread::sleep(check_timer);
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
                            .or_insert(Duration::default()) += check_timer;
                    }
                };

                if last_save.elapsed() > save_timer {
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
        let graph_data = graph_data.clone();
        let input_stats = input_stats_data.clone();
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
                let data: HashMap<String, Duration> =
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
