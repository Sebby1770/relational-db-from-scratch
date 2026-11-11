use crate::geometry::Ray;
use glam::{Vec3, vec3};

#[derive(Clone, Copy)]
pub struct Camera {
    yaw: f32,
    elevation: f32,
    radius: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.29,
            elevation: 0.19,
            radius: 8.9,
        }
    }
}

impl Camera {
    pub fn orbit(&mut self, yaw: f32, elevation: f32) {
        self.yaw += yaw;
        self.elevation = (self.elevation + elevation).clamp(0.015, 1.38);
    }

    pub fn zoom(&mut self, steps: f32) {
        self.radius = (self.radius * (-steps * 0.10).exp()).clamp(5.2, 17.0);
    }

    pub fn view(self, width: usize, height: usize) -> View {
        let (sin_yaw, cos_yaw) = self.yaw.sin_cos();
        let (sin_el, cos_el) = self.elevation.sin_cos();
        let back = vec3(sin_yaw * cos_el, sin_el, cos_yaw * cos_el);
        let right = vec3(cos_yaw, 0.0, -sin_yaw);
        let up = back.cross(right);
        let pixel = 2.0 * (18.0_f32.to_radians()).tan() / height as f32;
        View {
            origin: vec3(0.0, 1.53, 0.0) + self.radius * back,
            top_left: -back + pixel * (up * height as f32 - right * width as f32) * 0.5,
            dx: right * pixel,
            dy: -up * pixel,
        }
    }
}

pub struct View {
    origin: Vec3,
    top_left: Vec3,
    dx: Vec3,
    dy: Vec3,
}

impl View {
    pub fn ray(&self, x: f32, y: f32) -> Ray {
        Ray {
            origin: self.origin,
            direction: (self.top_left + x * self.dx + y * self.dy).normalize(),
        }
    }
}
