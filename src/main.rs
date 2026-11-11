mod camera;
mod geometry;
mod render;
mod scene;

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::time::Instant;

use camera::Camera;
use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use render::{Frame, SAMPLE_LIMIT, START_SUN};
use scene::Scene;

const HELP: &str = "Meridian | Real-Time Ray Tracer

  real-time-ray-tracer                 Open the interactive window
  real-time-ray-tracer --render FILE   Render the default scene to a 1200 x 750 PNG
  real-time-ray-tracer --help          Show this help

Drag or use arrow keys to orbit. Scroll or use W/S to zoom.
Space animates or pauses the sun. 1/2/3 select render quality.
R resets the view and sun. Esc closes the window.";

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [] => run_window(),
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{HELP}");
            Ok(())
        }
        [flag, path] if flag == "--render" && !path.is_empty() => {
            let mut frame = Frame::new(1200, 750);
            let scene = Scene::default();
            let start = Instant::now();
            for _ in 0..SAMPLE_LIMIT {
                frame.sample(&scene, Camera::default(), START_SUN);
            }
            write_png(&frame, Path::new(path))?;
            println!(
                "Saved {} in {:.2}s",
                Path::new(path).display(),
                start.elapsed().as_secs_f32()
            );
            Ok(())
        }
        _ => Err(format!("Invalid arguments.\n\n{HELP}").into()),
    }
}

fn write_png(frame: &Frame, path: &Path) -> Result<(), Box<dyn Error>> {
    let output = BufWriter::new(File::create(path)?);
    let mut encoder = png::Encoder::new(output, frame.width as u32, frame.height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    let mut rgb = Vec::with_capacity(frame.pixels.len() * 3);
    for pixel in &frame.pixels {
        rgb.extend_from_slice(&pixel.to_be_bytes()[1..]);
    }
    writer.write_image_data(&rgb)?;
    writer.finish()?;
    Ok(())
}

struct Viewer {
    camera: Camera,
    sun: f32,
    animate: bool,
    last_pointer: Option<(f32, f32)>,
    max_width: usize,
}

impl Default for Viewer {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            sun: START_SUN,
            animate: false,
            last_pointer: None,
            max_width: 720,
        }
    }
}

impl Viewer {
    fn handle_input(&mut self, window: &Window, dt: f32) -> bool {
        let mut changed = false;
        if window.is_key_pressed(Key::R, KeyRepeat::No) {
            self.camera = Camera::default();
            self.sun = START_SUN;
            self.animate = false;
            changed = true;
        }
        if window.is_key_pressed(Key::Space, KeyRepeat::No) {
            self.animate = !self.animate;
        }
        for (key, cap) in [(Key::Key1, 480), (Key::Key2, 720), (Key::Key3, 1200)] {
            if window.is_key_pressed(key, KeyRepeat::No) {
                self.max_width = cap;
            }
        }
        let axis = |positive, negative| {
            (window.is_key_down(positive) as i8 - window.is_key_down(negative) as i8) as f32
        };
        let mut yaw = dt * axis(Key::Right, Key::Left);
        let mut pitch = dt * axis(Key::Up, Key::Down);
        let pointer = if window.get_mouse_down(MouseButton::Left) {
            window.get_mouse_pos(MouseMode::Clamp)
        } else {
            None
        };
        if let (Some((x, y)), Some((last_x, last_y))) = (pointer, self.last_pointer) {
            yaw -= (x - last_x) * 0.007;
            pitch += (y - last_y) * 0.005;
        }
        self.last_pointer = pointer;
        if yaw != 0.0 || pitch != 0.0 {
            self.camera.orbit(yaw, pitch);
            changed = true;
        }
        let zoom =
            window.get_scroll_wheel().map_or(0.0, |(_, dy)| dy) + dt * 8.0 * axis(Key::W, Key::S);
        if zoom != 0.0 {
            self.camera.zoom(zoom);
            changed = true;
        }
        if self.animate {
            self.sun += dt * 0.48;
        }
        changed || self.animate
    }

    fn dimensions(&self, size: (usize, usize)) -> (usize, usize) {
        let (width, height) = (size.0.max(1), size.1.max(1));
        // Bound work in both landscape and portrait windows.
        let scale = (self.max_width as f32 / width.max(height) as f32).min(1.0);
        (
            ((width as f32 * scale).round() as usize).max(1),
            ((height as f32 * scale).round() as usize).max(1),
        )
    }
}

fn run_window() -> Result<(), Box<dyn Error>> {
    let mut window = Window::new(
        "Meridian | Real-Time Ray Tracer",
        960,
        600,
        WindowOptions {
            resize: true,
            ..WindowOptions::default()
        },
    )?;
    println!("{HELP}");
    let scene = Scene::default();
    let mut viewer = Viewer::default();
    let (width, height) = viewer.dimensions(window.get_size());
    let mut frame = Frame::new(width, height);
    let mut last_frame = Instant::now();
    let mut last_title = None;
    window.set_target_fps(60);
    while window.is_open() && !window.is_key_down(Key::Escape) {
        let now = Instant::now();
        let dt = now.duration_since(last_frame).as_secs_f32().min(0.1);
        last_frame = now;
        let changed = viewer.handle_input(&window, dt);
        let (width, height) = viewer.dimensions(window.get_size());
        if (width, height) != (frame.width, frame.height) {
            frame = Frame::new(width, height);
        } else if changed {
            frame.restart();
        }
        if frame.samples < SAMPLE_LIMIT {
            frame.sample(&scene, viewer.camera, viewer.sun);
        }
        let state = if viewer.animate {
            "Sun moving"
        } else {
            "Sun paused"
        };
        let title = (width, height, viewer.animate);
        if last_title != Some(title) {
            window.set_title(&format!("Meridian  |  {width} × {height}  |  Drag to orbit · Scroll to zoom · Space: {state}"));
            last_title = Some(title);
        }
        window.update_with_buffer(&frame.pixels, width, height)?;
    }
    Ok(())
}
