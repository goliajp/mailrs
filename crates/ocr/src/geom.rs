//! Plane geometry the detector's post-processing needs: convex hull,
//! minimum-area rectangle, corner ordering and the 3x3 homography that maps
//! a text quad onto an upright crop

/// A point in pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    /// Horizontal, growing rightwards.
    pub x: f32,
    /// Vertical, growing downwards.
    pub y: f32,
}

fn cross(o: Pt, a: Pt, b: Pt) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Convex hull, counter-clockwise, collinear points dropped.
pub(crate) fn convex_hull(mut pts: Vec<Pt>) -> Vec<Pt> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut hull = half_hull(pts.iter());
    hull.extend(half_hull(pts.iter().rev()));
    hull
}

/// One chain of Andrew's monotone chain, without its last point (it opens
/// the other chain).
fn half_hull<'a>(pts: impl Iterator<Item = &'a Pt>) -> Vec<Pt> {
    let mut chain: Vec<Pt> = Vec::new();
    for &p in pts {
        while chain.len() >= 2 && cross(chain[chain.len() - 2], chain[chain.len() - 1], p) <= 0.0 {
            chain.pop();
        }
        chain.push(p);
    }
    chain.pop();
    chain
}

/// A rotated rectangle: centre, side lengths and the direction of the
/// `w` side.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RotRect {
    pub cx: f32,
    pub cy: f32,
    pub w: f32,
    pub h: f32,
    /// Unit vector along the `w` side.
    pub ux: f32,
    pub uy: f32,
}

impl RotRect {
    pub(crate) fn short_side(&self) -> f32 {
        self.w.min(self.h)
    }

    /// The same rectangle grown by `d` on every side.
    pub(crate) fn grown(&self, d: f32) -> RotRect {
        RotRect {
            w: self.w + 2.0 * d,
            h: self.h + 2.0 * d,
            ..*self
        }
    }

    pub(crate) fn corners(&self) -> [Pt; 4] {
        let (hx, hy) = (self.ux * self.w / 2.0, self.uy * self.w / 2.0);
        let (vx, vy) = (-self.uy * self.h / 2.0, self.ux * self.h / 2.0);
        [
            Pt {
                x: self.cx - hx - vx,
                y: self.cy - hy - vy,
            },
            Pt {
                x: self.cx + hx - vx,
                y: self.cy + hy - vy,
            },
            Pt {
                x: self.cx + hx + vx,
                y: self.cy + hy + vy,
            },
            Pt {
                x: self.cx - hx + vx,
                y: self.cy - hy + vy,
            },
        ]
    }
}

/// Smallest-area enclosing rectangle of a convex hull (rotating calipers:
/// one side of the optimum lies on a hull edge).
pub(crate) fn min_area_rect(hull: &[Pt]) -> RotRect {
    if hull.len() < 3 {
        return axis_box(hull);
    }
    let mut best: Option<(f32, RotRect)> = None;
    for i in 0..hull.len() {
        let a = hull[i];
        let b = hull[(i + 1) % hull.len()];
        let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        if len == 0.0 {
            continue;
        }
        let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
        let (mut lo_u, mut hi_u, mut lo_v, mut hi_v) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in hull {
            let u = p.x * ux + p.y * uy;
            let v = -p.x * uy + p.y * ux;
            lo_u = lo_u.min(u);
            hi_u = hi_u.max(u);
            lo_v = lo_v.min(v);
            hi_v = hi_v.max(v);
        }
        let (w, h) = (hi_u - lo_u, hi_v - lo_v);
        let area = w * h;
        if best.as_ref().is_none_or(|(a, _)| area < *a) {
            let (cu, cv) = ((lo_u + hi_u) / 2.0, (lo_v + hi_v) / 2.0);
            let rect = RotRect {
                cx: cu * ux - cv * uy,
                cy: cu * uy + cv * ux,
                w,
                h,
                ux,
                uy,
            };
            best = Some((area, rect));
        }
    }
    best.map(|(_, r)| r).unwrap_or_else(|| axis_box(hull))
}

fn axis_box(pts: &[Pt]) -> RotRect {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in pts {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    RotRect {
        cx: (x0 + x1) / 2.0,
        cy: (y0 + y1) / 2.0,
        w: x1 - x0,
        h: y1 - y0,
        ux: 1.0,
        uy: 0.0,
    }
}

/// Corners as top-left, top-right, bottom-right, bottom-left: the two
/// leftmost by x are the left side, each side ordered by y.
pub(crate) fn order_corners(mut p: [Pt; 4]) -> [Pt; 4] {
    p.sort_by(|a, b| a.x.total_cmp(&b.x));
    let mut left = [p[0], p[1]];
    let mut right = [p[2], p[3]];
    left.sort_by(|a, b| a.y.total_cmp(&b.y));
    right.sort_by(|a, b| a.y.total_cmp(&b.y));
    [left[0], right[0], right[1], left[1]]
}

/// Homography taking `dst[i]` to `src[i]`, row-major with h33 = 1: what a
/// crop needs to look up, for each output pixel, where it came from.
pub(crate) fn homography(dst: [Pt; 4], src: [Pt; 4]) -> Option<[f32; 9]> {
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = (f64::from(dst[i].x), f64::from(dst[i].y));
        let (u, v) = (f64::from(src[i].x), f64::from(src[i].y));
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for col in 0..8 {
        let pivot = (col..8).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let lead = m[col];
        for (row, r) in m.iter_mut().enumerate() {
            if row == col {
                continue;
            }
            let f = r[col] / lead[col];
            for (a, b) in r.iter_mut().zip(lead.iter()).skip(col) {
                *a -= f * b;
            }
        }
    }
    let mut h = [0f32; 9];
    for i in 0..8 {
        h[i] = (m[i][8] / m[i][i]) as f32;
    }
    h[8] = 1.0;
    Some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    #[test]
    fn hull_drops_inner_and_collinear_points() {
        let pts = vec![
            p(0.0, 0.0),
            p(2.0, 0.0),
            p(4.0, 0.0),
            p(4.0, 4.0),
            p(0.0, 4.0),
            p(2.0, 2.0),
        ];
        assert_eq!(convex_hull(pts).len(), 4);
    }

    #[test]
    fn min_rect_of_a_tilted_square_follows_its_edges() {
        let hull = convex_hull(vec![p(5.0, 0.0), p(10.0, 5.0), p(5.0, 10.0), p(0.0, 5.0)]);
        let r = min_area_rect(&hull);
        let side = 50f32.sqrt();
        assert!(
            (r.w - side).abs() < 1e-3 && (r.h - side).abs() < 1e-3,
            "{r:?}"
        );
        assert!((r.cx - 5.0).abs() < 1e-3 && (r.cy - 5.0).abs() < 1e-3);
    }

    #[test]
    fn corners_come_out_clockwise_from_top_left() {
        let c = order_corners([p(10.0, 5.0), p(0.0, 0.0), p(0.0, 5.0), p(10.0, 0.0)]);
        assert_eq!(c, [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 5.0), p(0.0, 5.0)]);
    }

    #[test]
    fn homography_maps_each_corner_back() {
        let dst = [p(0.0, 0.0), p(100.0, 0.0), p(100.0, 20.0), p(0.0, 20.0)];
        let src = [p(10.0, 30.0), p(108.0, 50.0), p(104.0, 70.0), p(6.0, 50.0)];
        let h = homography(dst, src).unwrap();
        for i in 0..4 {
            let (x, y) = (dst[i].x, dst[i].y);
            let w = h[6] * x + h[7] * y + h[8];
            let (u, v) = (
                (h[0] * x + h[1] * y + h[2]) / w,
                (h[3] * x + h[4] * y + h[5]) / w,
            );
            assert!(
                (u - src[i].x).abs() < 1e-2 && (v - src[i].y).abs() < 1e-2,
                "{i}: {u},{v}"
            );
        }
    }
}
