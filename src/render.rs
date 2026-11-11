use crate::{
    camera::Camera,
    scene::{Lighting, Scene},
};
use glam::{Vec2, Vec3};
use rayon::prelude::*;
use std::f32::consts::TAU;

pub const SAMPLE_LIMIT: u32 = 64;
pub const START_SUN: f32 = 2.36;

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    sum: Vec<Vec3>,
    pub samples: u32,
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
            sum: vec![Vec3::ZERO; width * height],
            samples: 0,
        }
    }

    pub fn restart(&mut self) {
        // The next pass replaces every sum, so camera motion needs no extra clear.
        self.samples = 0;
    }

    pub fn sample(&mut self, scene: &Scene, camera: Camera, sun: f32) {
        let pass = self.samples;
        let (pixel_offset, disk) = if pass == 0 {
            (Vec2::splat(0.5), Vec2::ZERO)
        } else {
            let angle = TAU * radical_inverse(pass, 5);
            let radius = radical_inverse(pass, 7).sqrt();
            (
                Vec2::new(radical_inverse(pass, 2), radical_inverse(pass, 3)),
                radius * Vec2::new(angle.cos(), angle.sin()),
            )
        };
        let light = Lighting::new(sun, disk);
        let view = camera.view(self.width, self.height);
        let inv_count = 1.0 / (pass + 1) as f32;
        self.sum
            .par_chunks_mut(self.width)
            .zip(self.pixels.par_chunks_mut(self.width))
            .enumerate()
            .for_each(|(y, (sums, pixels))| {
                for (x, (sum, pixel)) in sums.iter_mut().zip(pixels).enumerate() {
                    let ray = view.ray(x as f32 + pixel_offset.x, y as f32 + pixel_offset.y);
                    let sample = scene.trace(ray, &light, 2);
                    *sum = if pass == 0 { sample } else { *sum + sample };
                    *pixel = pack(*sum * inv_count);
                }
            });
        self.samples += 1;
    }
}

// Halton sequence: each prefix distributes samples over the pixel and the light.
fn radical_inverse(mut number: u32, base: u32) -> f32 {
    let mut value = 0.0;
    let mut weight = 1.0 / base as f32;
    while number > 0 {
        value += (number % base) as f32 * weight;
        number /= base;
        weight /= base as f32;
    }
    value
}

fn pack(linear: Vec3) -> u32 {
    let mapped = (linear * (2.51 * linear + 0.03) / (linear * (2.43 * linear + 0.59) + 0.14))
        .clamp(Vec3::ZERO, Vec3::ONE);
    // A square-root display curve keeps the per-pixel conversion inexpensive.
    let bytes =
        (Vec3::new(mapped.x.sqrt(), mapped.y.sqrt(), mapped.z.sqrt()) * 255.0 + 0.5).as_uvec3();
    (bytes.x << 16) | (bytes.y << 8) | bytes.z
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_camera_and_sun_changes_image_and_discards_old_samples() {
        let scene = Scene::default();
        let mut camera = Camera::default();
        let mut frame = Frame::new(48, 32);
        frame.sample(&scene, camera, START_SUN);
        let original = frame.pixels.clone();
        assert!(original.iter().any(|p| *p != original[0]));
        frame.sample(&scene, camera, START_SUN);
        assert_ne!(original, frame.pixels);
        frame.restart();
        frame.sample(&scene, camera, START_SUN);
        assert_eq!(original, frame.pixels);
        frame.restart();
        camera.orbit(0.4, 0.1);
        frame.sample(&scene, camera, START_SUN);
        assert_ne!(original, frame.pixels);
        let new_view = frame.pixels.clone();
        frame.restart();
        frame.sample(&scene, camera, START_SUN + 1.0);
        assert_ne!(new_view, frame.pixels);
    }
}
