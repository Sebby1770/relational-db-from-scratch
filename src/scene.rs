use crate::geometry::{EPSILON, Hit, Ray, Shape};
use glam::{Vec2, Vec3, vec3};

const RING_CENTER: Vec3 = vec3(-1.02, 1.92, -0.55);

#[derive(Clone, Copy)]
enum Finish {
    Sandstone,
    Clay,
    Chrome,
    Enamel,
    Brass,
    Tile,
}

impl Finish {
    fn albedo(self, p: Vec3) -> Vec3 {
        match self {
            Self::Sandstone => {
                let layer = p.y * 96.0 + p.x * 2.7 + (p.z * 3.8).sin();
                let grain = 0.96 + 0.025 * layer.sin() + 0.015 * (layer * 2.4).sin();
                vec3(0.84, 0.69, 0.50) * grain
            }
            Self::Clay => vec3(0.70, 0.155, 0.046),
            Self::Chrome => vec3(0.93, 0.965, 1.0),
            Self::Enamel => vec3(0.025, 0.14, 0.135),
            Self::Brass => vec3(0.93, 0.59, 0.22),
            Self::Tile => {
                let tile = Vec2::new(p.x, p.z) / 2.4;
                let local = tile - tile.floor() - Vec2::splat(0.5);
                let edge = 0.5 - local.abs().max_element();
                let seam = (edge * 165.0).min(1.0);
                let alternate = (tile.x.floor() + tile.y.floor()).rem_euclid(2.0);
                vec3(0.62, 0.52, 0.40) * (0.93 + 0.07 * alternate) * (0.79 + 0.21 * seam)
            }
        }
    }
}

pub struct Scene {
    objects: [(Shape, Finish); 8],
}

impl Default for Scene {
    fn default() -> Self {
        use Finish::*;
        use Shape::*;
        Self {
            objects: [
                (
                    Ring {
                        center: RING_CENTER,
                        inner: 1.07,
                        outer: 1.67,
                        half_depth: 0.32,
                    },
                    Clay,
                ),
                (
                    Block {
                        min: vec3(-2.50, 0.0, -1.20),
                        max: vec3(0.45, 0.42, 0.02),
                    },
                    Sandstone,
                ),
                (
                    Block {
                        min: vec3(0.82, 0.0, -1.83),
                        max: vec3(2.05, 1.60, -0.60),
                    },
                    Enamel,
                ),
                (
                    Sphere {
                        center: vec3(1.43, 2.16, -1.22),
                        radius: 0.56,
                    },
                    Brass,
                ),
                (
                    Block {
                        min: vec3(0.15, 0.0, 0.34),
                        max: vec3(2.10, 0.20, 2.02),
                    },
                    Sandstone,
                ),
                (
                    Sphere {
                        center: vec3(1.09, 1.0, 1.13),
                        radius: 0.8,
                    },
                    Chrome,
                ),
                (
                    Sphere {
                        center: vec3(-1.48, 0.37, 1.28),
                        radius: 0.37,
                    },
                    Enamel,
                ),
                (Ground, Tile),
            ],
        }
    }
}

/// Sampling directions are shared by a pass. A resting view integrates the solar
/// disk over several passes, while the first (interactive) pass uses its center.
pub struct Lighting {
    sun: Vec3,
    shadow_direction: Vec3,
    hemisphere: Vec3,
}

impl Lighting {
    pub fn new(azimuth: f32, disk: Vec2) -> Self {
        let sun = vec3(azimuth.cos(), 0.85, azimuth.sin()).normalize();
        let tangent = Vec3::Y.cross(sun).normalize();
        let bitangent = sun.cross(tangent);
        let shadow_direction = (sun + 0.055 * (disk.x * tangent + disk.y * bitangent)).normalize();
        Self {
            sun,
            shadow_direction,
            hemisphere: disk.extend((1.0 - disk.length_squared()).max(0.0).sqrt()),
        }
    }

    fn sky(&self, direction: Vec3) -> Vec3 {
        let altitude = (2.3 * direction.y).clamp(0.0, 1.0).sqrt();
        let sky = vec3(0.94, 0.86, 0.71).lerp(vec3(0.17, 0.44, 0.63), altitude);
        let alignment = direction.dot(self.sun).max(0.0);
        sky + vec3(1.0, 0.82, 0.53) * (0.24 * alignment.powi(12) + 8.0 * alignment.powi(1600))
    }
}

impl Scene {
    fn nearest(&self, ray: Ray, mut limit: f32) -> Option<(Hit, Finish)> {
        let mut result = None;
        for &(shape, finish) in &self.objects {
            if let Some(hit) = shape.hit(ray, limit) {
                limit = hit.distance;
                result = Some((hit, finish));
            }
        }
        result
    }

    fn blocked(&self, ray: Ray) -> bool {
        self.objects
            .iter()
            .any(|(shape, _)| shape.hit(ray, f32::INFINITY).is_some())
    }

    pub fn trace(&self, ray: Ray, light: &Lighting, remaining_bounces: u8) -> Vec3 {
        let Some((hit, finish)) = self.nearest(ray, f32::INFINITY) else {
            return light.sky(ray.direction);
        };
        let point = ray.at(hit.distance);
        let outward = point + hit.normal * (EPSILON * 3.0);
        // Shallow concentric tool marks on the two flat clay faces.
        let n = if matches!(finish, Finish::Clay) && hit.normal.z.abs() > 0.99 {
            let radial = (point - RING_CENTER).truncate();
            let slope = 0.21 * (radial.length() * 118.0).sin();
            (hit.normal + slope * radial.normalize().extend(0.0)).normalize()
        } else {
            hit.normal
        };
        let albedo = finish.albedo(point);
        let cosine = n.dot(light.shadow_direction).max(0.0);
        let sunlit = cosine > 0.0
            && !self.blocked(Ray {
                origin: outward,
                direction: light.shadow_direction,
            });
        let direct = if sunlit {
            vec3(1.30, 1.14, 0.93) * cosine
        } else {
            Vec3::ZERO
        };
        let ambient = vec3(0.22, 0.24, 0.26) * (0.77 + 0.23 * n.y);
        // Probe the nearby hemisphere for ambient occlusion. Distance weighting
        // makes contact shadows fade continuously towards open surfaces.
        let mut visibility = 1.0;
        if light.hemisphere.z < 1.0 && hit.distance < 24.0 {
            let normal = hit.normal;
            let axis = if normal.y.abs() < 0.9 {
                Vec3::Y
            } else {
                Vec3::Z
            };
            let tangent = axis.cross(normal).normalize();
            let direction = tangent * light.hemisphere.x
                + normal.cross(tangent) * light.hemisphere.y
                + normal * light.hemisphere.z;
            if let Some((blocker, _)) = self.nearest(
                Ray {
                    origin: outward,
                    direction,
                },
                1.8,
            ) {
                visibility -= 0.88 * (1.0 - blocker.distance / 1.8);
            }
        }
        let diffuse = albedo * (ambient * visibility + direct);

        let color = match finish {
            Finish::Chrome | Finish::Brass => {
                let reflected = Ray {
                    origin: outward,
                    direction: ray.direction.reflect(n),
                };
                let incoming = if remaining_bounces > 0 {
                    self.trace(reflected, light, remaining_bounces - 1)
                } else {
                    light.sky(reflected.direction)
                };
                let fresnel = (1.0 + ray.direction.dot(n)).clamp(0.0, 1.0).powi(5);
                incoming * albedo.lerp(Vec3::ONE, fresnel) + 0.035 * diffuse
            }
            Finish::Enamel if sunlit => {
                let halfway = (light.shadow_direction - ray.direction).normalize_or_zero();
                diffuse + Vec3::splat(0.48 * n.dot(halfway).max(0.0).powi(70))
            }
            _ => diffuse,
        };
        // Fade the infinite floor into the horizon without washing out nearby objects.
        let haze = 1.0 - (-0.025 * (hit.distance - 15.0).max(0.0)).exp();
        color.lerp(vec3(0.94, 0.86, 0.71), haze)
    }
}
