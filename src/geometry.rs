use glam::{Vec3, vec3};

pub const EPSILON: f32 = 0.0005;

#[derive(Clone, Copy)]
pub struct Ray {
    pub origin: Vec3,
    /// Unit direction, so intersection distances are in world units.
    pub direction: Vec3,
}

impl Ray {
    pub fn at(self, distance: f32) -> Vec3 {
        self.origin + distance * self.direction
    }
}

#[derive(Clone, Copy)]
pub struct Hit {
    pub distance: f32,
    pub normal: Vec3,
}

#[derive(Clone, Copy)]
pub enum Shape {
    Sphere {
        center: Vec3,
        radius: f32,
    },
    Block {
        min: Vec3,
        max: Vec3,
    },
    /// A hollow cylinder along Z, including both annular end caps.
    Ring {
        center: Vec3,
        inner: f32,
        outer: f32,
        half_depth: f32,
    },
    Ground,
}

impl Shape {
    pub fn hit(self, ray: Ray, limit: f32) -> Option<Hit> {
        match self {
            Self::Sphere { center, radius } => sphere(ray, center, radius, limit),
            Self::Block { min, max } => block(ray, min, max, limit),
            Self::Ring {
                center,
                inner,
                outer,
                half_depth,
            } => ring(ray, center, inner, outer, half_depth, limit),
            Self::Ground if ray.direction.y < -1e-6 => {
                let distance = -ray.origin.y / ray.direction.y;
                in_range(distance, limit).then_some(Hit {
                    distance,
                    normal: Vec3::Y,
                })
            }
            Self::Ground => None,
        }
    }
}

fn in_range(distance: f32, limit: f32) -> bool {
    distance > EPSILON && distance < limit
}

fn sphere(ray: Ray, center: Vec3, radius: f32, limit: f32) -> Option<Hit> {
    let offset = ray.origin - center;
    let half_b = offset.dot(ray.direction);
    let discriminant = half_b * half_b - (offset.length_squared() - radius * radius);
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let distance = [-half_b - root, -half_b + root]
        .into_iter()
        .find(|&t| in_range(t, limit))?;
    Some(Hit {
        distance,
        normal: (ray.at(distance) - center) / radius,
    })
}

fn block(ray: Ray, min: Vec3, max: Vec3, limit: f32) -> Option<Hit> {
    // Intersect the three slabs, retaining both faces for rays starting inside.
    let mut entry = Hit {
        distance: f32::NEG_INFINITY,
        normal: Vec3::ZERO,
    };
    let mut exit = Hit {
        distance: f32::INFINITY,
        normal: Vec3::ZERO,
    };
    for (axis, normal) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
        let origin = ray.origin[axis];
        let direction = ray.direction[axis];
        if direction.abs() < 1e-7 {
            if origin < min[axis] || origin > max[axis] {
                return None;
            }
            continue;
        }
        let a = (min[axis] - origin) / direction;
        let b = (max[axis] - origin) / direction;
        let outward = direction.signum() * normal;
        if a.min(b) > entry.distance {
            entry = Hit {
                distance: a.min(b),
                normal: -outward,
            };
        }
        if a.max(b) < exit.distance {
            exit = Hit {
                distance: a.max(b),
                normal: outward,
            };
        }
        if entry.distance > exit.distance || entry.distance >= limit || exit.distance <= EPSILON {
            return None;
        }
    }
    [entry, exit]
        .into_iter()
        .find(|hit| in_range(hit.distance, limit))
}

fn ring(
    ray: Ray,
    center: Vec3,
    inner: f32,
    outer: f32,
    half_depth: f32,
    limit: f32,
) -> Option<Hit> {
    let origin = ray.origin - center;
    let direction = ray.direction;
    let mut result = None;
    let mut nearest = limit;
    let mut record = |distance, normal| {
        if in_range(distance, nearest) {
            nearest = distance;
            result = Some(Hit { distance, normal });
        }
    };

    if direction.z.abs() > 1e-7 {
        for side in [-1.0, 1.0] {
            let distance = (side * half_depth - origin.z) / direction.z;
            let radial_sq = (origin + distance * direction).truncate().length_squared();
            if radial_sq >= inner * inner && radial_sq <= outer * outer {
                record(distance, side * Vec3::Z);
            }
        }
    }

    let a = direction.truncate().length_squared();
    if a > 1e-10 {
        let half_b = origin.truncate().dot(direction.truncate());
        for (radius, sign) in [(outer, 1.0), (inner, -1.0)] {
            let c = origin.truncate().length_squared() - radius * radius;
            let discriminant = half_b * half_b - a * c;
            if discriminant < 0.0 {
                continue;
            }
            let root = discriminant.sqrt();
            for distance in [(-half_b - root) / a, (-half_b + root) / a] {
                let p = origin + distance * direction;
                if p.z.abs() <= half_depth {
                    record(distance, vec3(p.x, p.y, 0.0) * (sign / radius));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray {
            origin,
            direction: direction.normalize(),
        }
    }

    #[test]
    fn sphere_uses_nearest_forward_root_and_respects_limit() {
        let shape = Shape::Sphere {
            center: Vec3::ZERO,
            radius: 1.0,
        };
        let incoming = ray(3.0 * Vec3::Z, -Vec3::Z);
        assert_eq!(shape.hit(incoming, 10.0).unwrap().distance, 2.0);
        assert!(shape.hit(incoming, 1.9).is_none());
        assert_eq!(
            shape.hit(ray(Vec3::ZERO, Vec3::X), 2.0).unwrap().normal,
            Vec3::X
        );
    }

    #[test]
    fn parallel_slab_rays_handle_misses_and_inside_exits() {
        let cube = Shape::Block {
            min: -Vec3::ONE,
            max: Vec3::ONE,
        };
        assert!(cube.hit(ray(2.0 * Vec3::Y, Vec3::X), 10.0).is_none());
        let hit = cube.hit(ray(Vec3::ZERO, -Vec3::X), 10.0).unwrap();
        assert_eq!((hit.distance, hit.normal), (1.0, -Vec3::X));
    }

    #[test]
    fn ring_has_an_open_center_end_caps_and_inward_bore_normal() {
        let shape = Shape::Ring {
            center: Vec3::ZERO,
            inner: 1.0,
            outer: 2.0,
            half_depth: 0.5,
        };
        assert!(shape.hit(ray(3.0 * Vec3::Z, -Vec3::Z), 10.0).is_none());
        let cap = shape.hit(ray(vec3(1.5, 0.0, 3.0), -Vec3::Z), 10.0).unwrap();
        assert_eq!((cap.distance, cap.normal), (2.5, Vec3::Z));
        let bore = shape.hit(ray(Vec3::ZERO, Vec3::X), 10.0).unwrap();
        assert_eq!((bore.distance, bore.normal), (1.0, -Vec3::X));
    }
}
