#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{time::{Duration, Instant}, thread, fs::File, sync::{mpsc::{Receiver, Sender}, atomic::{AtomicBool, Ordering}, Arc}, collections::VecDeque};
use egui::ProgressBar;
use gif::{Encoder, Repeat, Frame};
use scrap::{Display, Capturer};
use tokio::runtime::Runtime;

fn main() {

    let rt = Runtime::new().expect("Unable to create Runtime");

    // Enter the runtime so that `tokio::spawn` is available immediately.
    let _enter = rt.enter();

    // Execute the runtime in its own thread.
    // The future doesn't have to do anything. In this example, it just sleeps forever.
    std::thread::spawn(move || {
        rt.block_on(async {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
        })
    });

    let options = eframe::NativeOptions {
        initial_window_size: Some(egui::vec2(300.0, 230.0)),
        icon_data: Some(load_icon(".\\icon.png")),
        ..Default::default()
    };
    eframe::run_native(
        "Clipper",
        options,
        Box::new(|_cc| Box::new(Clipper::default())),
    );
}

fn load_icon(path: &str) -> eframe::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::open(path)
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    eframe::IconData {
        rgba: icon_rgba,
        width: icon_width,
        height: icon_height,
    }
}

struct Clipper {
    async_to_ui: (Sender<State>, Receiver<State>),
    path: String,
    recording: Arc<AtomicBool>,
    clip_length: usize,
    fps: u8,
    current: State,
    quality: Quality,
}

#[derive(PartialEq, Clone)]
enum State {
    Idle, Recording, Converting(f32), Encoding(f32)
}

#[derive(PartialEq, Clone, Debug)]
enum Quality {
    Original, Half
}

trait Encode {
    fn encode(&self, frames: Vec<Vec<u8>>, target: File, original_width: usize, original_height: usize, send: Sender<State>, fps: u8);
}

impl Encode for Quality {
    fn encode(&self, frames: Vec<Vec<u8>>, target: File, original_width: usize, original_height: usize, send: Sender<State>, fps: u8) {
        match self {
            Quality::Original => {
                let frame_count = frames.len() as f32;
                let incr = frame_count / 100.0 / 100.0;
                let mut current = 0.0;
                let _ = send.send(State::Encoding(current));

                let width = original_width as u16;
                let height = original_height as u16;

                let color_map = &[0xFF, 0xFF, 0xFF, 0, 0, 0];
                let mut image = target;
                let mut encoder = Encoder::new(&mut image, width, height, color_map).expect("Could not create encoder");
                encoder.set_repeat(Repeat::Infinite).expect("Could not set encoder property");
                for mut frame_data_single in frames {
                    let mut frame = Frame::from_rgba_speed(width, height, &mut frame_data_single, 30);
                    frame.delay = (100.0 / fps as f64) as u16;
                
                    frame.make_lzw_pre_encoded();
                    encoder.write_lzw_pre_encoded_frame(&frame).expect("Could not write frame to encoder");

                    current += incr;
                    let _ = send.send(State::Encoding(current));
                }
            },
            Quality::Half => {
                let frame_count = frames.len() as f32;
                let incr = frame_count / 100.0 / 100.0;
                let mut current = 0.0;

                let mut frame_data = vec![];
                for frame in frames {
                    let mut new_frame: Vec<u8> = Vec::with_capacity(frame.len());
                    let rows = frame.chunks(original_width * 4);
                    for (i, row) in rows.into_iter().enumerate() {
                        if i % 2 == 0 {
                            continue;
                        }
                        let mut row = row.chunks(4).into_iter().enumerate()
                            .filter(|(byte_ind, _)| byte_ind % 2 == 0)
                            .map(|(_, val)| {
                                val.to_vec()
                            })
                            .flatten()
                            .collect::<Vec<u8>>();
                        new_frame.append(&mut row);
                    }
                    frame_data.push(new_frame);
    
                    current += incr;
                    let _ = send.send(State::Converting(current));
                }

                current = 0.0;
                let _ = send.send(State::Encoding(current));

                let width = (original_width / 2) as u16;
                let height = (original_height / 2) as u16;

                let color_map = &[0xFF, 0xFF, 0xFF, 0, 0, 0];
                let mut image = target;
                let mut encoder = Encoder::new(&mut image, width, height, color_map).expect("Could not create encoder");
                encoder.set_repeat(Repeat::Infinite).expect("Could not set encoder property");
                for mut frame_data_single in frame_data {
                    let mut frame = Frame::from_rgba_speed(width, height, &mut frame_data_single, 30);
                    frame.delay = (100.0 / fps as f64) as u16;
                
                    frame.make_lzw_pre_encoded();
                    encoder.write_lzw_pre_encoded_frame(&frame).expect("Could not write frame to encoder");

                    current += incr;
                    let _ = send.send(State::Encoding(current));
                }
            },
        }
    }
}

impl Default for Clipper {
    fn default() -> Self {
        Self {
            async_to_ui: std::sync::mpsc::channel(),
            path: "wow.gif".to_string(),
            recording: Arc::new(AtomicBool::new(false)),
            clip_length: 5,
            fps: 30,
            current: State::Idle,
            quality: Quality::Half,
        }
    }
}

impl eframe::App for Clipper {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

        egui::CentralPanel::default().show(ctx, |ui| {

            ui.add_space(20.0);

            egui::Grid::new("my_grid")
                .num_columns(2)
                .spacing([40.0, 20.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label("File/path:");
                    ui.add(egui::TextEdit::singleline(&mut self.path).hint_text("File name/path"));
                    ui.end_row();
                    ui.label("Clip Length (seconds):");
                    ui.add(egui::DragValue::new(&mut self.clip_length).speed(1.0));
                    ui.end_row();
                    ui.label("FPS:");
                    egui::ComboBox::from_id_source("fps").selected_text(format!("{}", self.fps))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.fps, 10, "10");
                            ui.selectable_value(&mut self.fps, 20, "20");
                            ui.selectable_value(&mut self.fps, 25, "25");
                            ui.selectable_value(&mut self.fps, 33, "33");
                            ui.selectable_value(&mut self.fps, 50, "50");
                        });
                    ui.end_row();
                    ui.label("Quality:");
                    egui::ComboBox::from_id_source("quality").selected_text(format!("{:?}", self.quality))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.quality, Quality::Half, "Half");
                            ui.selectable_value(&mut self.quality, Quality::Original, "Original")
                        });
            });
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                let state = self.async_to_ui.1.try_recv().unwrap_or(self.current.clone());
                
                match state {
                    State::Idle => {
                        if ui.button("Start").clicked() {
                            self.recording.store(true, Ordering::SeqCst);
                            self.run();
                        }
                    },
                    State::Recording => {
                        ui.label("Recording...");
                        if ui.button("Stop").clicked() {
                            self.recording.store(false, Ordering::SeqCst);
                        }
                    },
                    State::Converting(v) => {
                        ui.label("Converting (this may take a while)...");
                        let proper = ((v * 100.0) as u32) as f32 / 100.0;
                        let progress_bar = ProgressBar::new(proper).show_percentage().animate(true);
                        ui.add(progress_bar);
                    },
                    State::Encoding(v) => {
                        ui.label("Encoding (this may take a while)...");
                        let proper = ((v * 100.0) as u32) as f32 / 100.0;
                        let progress_bar = ProgressBar::new(proper).show_percentage().animate(true);
                        ui.add(progress_bar);
                    },
                }
                self.current = state;
            });
        });
    }
}

impl Clipper {

    fn run(&mut self) {
        let path = self.path.clone();
        let clip_length = self.clip_length.clone();
        let fps = self.fps.clone();
        let send = self.async_to_ui.0.clone();
        let quality = self.quality.clone();

        let recording = Arc::clone(&self.recording);

        tokio::spawn(async move {
            let ms_per_frame = Duration::from_millis((1000.0 / fps as f64) as u64);
            let one_sixthyth = Duration::from_secs(1) / 60;

            let display = Display::primary().expect("Couldn't find primary display.");
            let mut capturer = Capturer::new(display).expect("Couldn't begin capture.");
            let (w, h) = (capturer.width(), capturer.height());

            let _ = send.send(State::Recording);

            let mut frame_data: VecDeque<Vec<u8>> = VecDeque::new();
            let mut instant = Instant::now();
            loop {
                if !recording.load(Ordering::SeqCst) {
                    break;
                }
                let frame = match capturer.frame() {
                    Ok(f) => f,
                    Err(error) => {
                        if error.kind() == std::io::ErrorKind::WouldBlock {
                            thread::sleep(one_sixthyth);
                            continue;
                        } else {
                            panic!("Error: {}", error);
                        }
                    }
                };
                let data = frame.to_vec();
                frame_data.push_back(data);
                if frame_data.len() > clip_length * fps as usize {
                    frame_data.pop_front();
                }
                let elapsed = instant.elapsed();
                if ms_per_frame > elapsed {
                    spin_sleep::sleep(ms_per_frame - elapsed);
                };
                instant = Instant::now();
            }

            let mut current = 0.0;
            let incr = frame_data.len() as f32 / 100.0 / 100.0;
            // flip BGRA to RGBA
            let frame_data = frame_data.iter().map(|frame| {
                current += incr;
                let _ = send.send(State::Converting(current));
                frame.chunks(4).into_iter().map(|byte| {
                    vec![byte[2], byte[1], byte[0], byte[3]]
                }).flatten().collect::<Vec<u8>>()
            }).collect::<Vec<Vec<u8>>>();

            let file = File::create(path.as_str()).expect("Could not create file");
            quality.encode(frame_data, file, w, h, send.clone(), fps);

            let _ = send.send(State::Idle);
        });
    }
}