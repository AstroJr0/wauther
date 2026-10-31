//! A concave corner: the inverse of a rounded one.
//!
//! A rounded corner cuts a quarter disc out of a container. A concave corner
use std::f32::consts::FRAC_PI_2;
use eframe::egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, pos2, vec2};

// arc segments. A quarter circle this coarse is smooth up to ~100px.
const SEGMENTS: u32 = 24;

// Which way the filled square lies from the corner it is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    NorthWest, //
    NorthEast,
    SouthWest,
    SouthEast,
}

impl Toward {
    /// `(dx, dy)`, each `-1.0` or `1.0`. y points down, as on screen.
    const fn signs(self) -> (f32, f32) {
        match self {
            Self::NorthWest => (-1.0, -1.0),
            Self::NorthEast => (1.0, -1.0),
            Self::SouthWest => (-1.0, 1.0),
            Self::SouthEast => (1.0, 1.0),
        }
    }
}

/// One concave corner, anchored at `at` and lying `toward` a direction.
///
/// Examples, for a container `c`:
/// - flush with the right edge, flaring up from its top-right corner:
///   `Concave::new(c.right_top(), Toward::NorthWest, ..)`
/// - hanging from the top, flaring down at its bottom-left corner:
///   `Concave::new(c.left_bottom(), Toward::SouthEast, ..)`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Concave {
    pub at: Pos2,
    pub toward: Toward,
    pub radius: f32,
    pub color: Color32,
}

impl Concave {
    #[must_use]
    pub const fn new(at: Pos2, toward: Toward, radius: f32, color: Color32) -> Self {
        Self {
            at,
            toward,
            radius,
            color,
        }
    }

    /// The square the piece fits in.
    #[must_use]
    pub fn square(&self) -> Rect {
        let (dx, dy) = self.toward.signs();
        let r = self.radius.max(0.0);
        Rect::from_two_pos(self.at, self.at + vec2(dx * r, dy * r))
    }

    /// The piece as triangles. Empty when the radius isn't positive.
    #[must_use]
    pub fn mesh(&self) -> Mesh {
        let mut mesh = Mesh::default();
        let r = self.radius;
        if r.is_nan() || r <= 0.0 {
            return mesh;
        }
        let (dx, dy) = self.toward.signs();
        // The quarter disc's centre is the square's far corner.
        let centre = self.at + vec2(dx * r, dy * r);
        mesh.colored_vertex(self.at, self.color);
        for k in 0..=SEGMENTS {
            let angle = FRAC_PI_2 * k as f32 / SEGMENTS as f32;
            let p = centre + vec2(-dx * r * angle.cos(), -dy * r * angle.sin());
            mesh.colored_vertex(p, self.color);
        }
        for k in 0..SEGMENTS {
            mesh.add_triangle(0, k + 1, k + 2);
        }
        mesh
    }

    pub fn paint(&self, painter: &Painter) {
        let mesh = self.mesh();
        if !mesh.is_empty() {
            painter.add(Shape::mesh(mesh));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Toward; 4] = [
        Toward::NorthWest,
        Toward::NorthEast,
        Toward::SouthWest,
        Toward::SouthEast,
    ];

    fn piece(toward: Toward, radius: f32) -> Concave {
        Concave::new(pos2(100.0, 50.0), toward, radius, Color32::WHITE)
    }

    fn area(mesh: &Mesh) -> f32 {
        mesh.indices
            .chunks_exact(3)
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize].pos);
                ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn the_square_lies_toward_its_direction() {
        let at = pos2(100.0, 50.0);

        let squares = ALL.map(|t| piece(t, 10.0).square());

        assert_eq!(squares[0], Rect::from_min_max(pos2(90.0, 40.0), at));
        assert_eq!(
            squares[1],
            Rect::from_min_max(pos2(100.0, 40.0), pos2(110.0, 50.0))
        );
        assert_eq!(
            squares[2],
            Rect::from_min_max(pos2(90.0, 50.0), pos2(100.0, 60.0))
        );
        assert_eq!(squares[3], Rect::from_min_max(at, pos2(110.0, 60.0)));
    }

    #[test]
    fn the_mesh_stays_inside_its_square() {
        for toward in ALL {
            let piece = piece(toward, 20.0);
            let square = piece.square().expand(0.001);

            let mesh = piece.mesh();

            assert!(
                mesh.vertices.iter().all(|v| square.contains(v.pos)),
                "{toward:?}"
            );
        }
    }

    #[test]
    fn the_arc_is_a_radius_away_from_the_far_corner() {
        for toward in ALL {
            let piece = piece(toward, 20.0);
            let (dx, dy) = toward.signs();
            let centre = piece.at + vec2(dx * 20.0, dy * 20.0);

            let mesh = piece.mesh();

            for v in &mesh.vertices[1..] {
                assert!(
                    ((v.pos - centre).length() - 20.0).abs() < 0.001,
                    "{toward:?}"
                );
            }
        }
    }

    #[test]
    fn the_arc_runs_from_edge_to_edge() {
        let piece = piece(Toward::NorthWest, 20.0);

        let mesh = piece.mesh();
        let (first, last) = (mesh.vertices[1].pos, mesh.vertices.last().unwrap().pos);

        // One end on the vertical side through `at`, the other on the horizontal one.
        assert!((first.x - 100.0).abs() < 0.001 && (first.y - 30.0).abs() < 0.001);
        assert!((last.y - 50.0).abs() < 0.001 && (last.x - 80.0).abs() < 0.001);
    }

    #[test]
    fn the_area_is_a_square_minus_a_quarter_disc() {
        let r = 20.0_f32;
        let exact = r * r * (1.0 - std::f32::consts::FRAC_PI_4);

        for toward in ALL {
            let got = area(&piece(toward, r).mesh());

            // The fan's chords sit just inside the arc, so it runs a hair large.
            assert!(
                got >= exact && got < exact * 1.01,
                "{toward:?}: {got} vs {exact}"
            );
        }
    }

    #[test]
    fn no_radius_no_mesh() {
        for radius in [0.0, -4.0, f32::NAN] {
            assert!(piece(Toward::NorthWest, radius).mesh().is_empty());
        }
    }

    #[test]
    fn every_triangle_uses_a_valid_vertex() {
        let mesh = piece(Toward::SouthEast, 12.0).mesh();

        assert_eq!(mesh.indices.len(), SEGMENTS as usize * 3);
        assert!(
            mesh.indices
                .iter()
                .all(|&i| (i as usize) < mesh.vertices.len())
        );
    }
}                                  
