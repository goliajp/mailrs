//! Post-processing for the DB (differentiable binarisation) text detector:
//! probability map to text quads in the coordinates of the image it ran on

use crate::geom::{Pt, convex_hull, min_area_rect, order_corners};

/// The detector's per-pixel text probability, row-major.
pub(crate) struct ProbMap<'a> {
    pub w: usize,
    pub h: usize,
    pub p: &'a [f32],
}

const THRESH: f32 = 0.3;
const BOX_THRESH: f32 = 0.5;
const UNCLIP_RATIO: f32 = 1.6;
const MIN_SIZE: f32 = 3.0;
const MAX_CANDIDATES: usize = 1000;

/// Text quads (top-left, top-right, bottom-right, bottom-left) scaled to a
/// `dest_w` x `dest_h` image, with their mean probability.
pub(crate) fn boxes(map: &ProbMap, dest_w: u32, dest_h: u32) -> Vec<([Pt; 4], f32)> {
    let mask = dilated_mask(map);
    let (sx, sy) = (dest_w as f32 / map.w as f32, dest_h as f32 / map.h as f32);
    let mut out = Vec::new();
    for edge in regions(&mask, map.w, map.h)
        .into_iter()
        .take(MAX_CANDIDATES)
    {
        let rect = min_area_rect(&convex_hull(edge));
        if rect.short_side() < MIN_SIZE {
            continue;
        }
        let score = mean_inside(map, rect.corners());
        if score < BOX_THRESH {
            continue;
        }
        let d = rect.w * rect.h * UNCLIP_RATIO / (2.0 * (rect.w + rect.h));
        let grown = rect.grown(d);
        if grown.short_side() < MIN_SIZE + 2.0 {
            continue;
        }
        let quad = grown.corners().map(|p| Pt {
            x: (p.x * sx).round().clamp(0.0, dest_w as f32 - 1.0),
            y: (p.y * sy).round().clamp(0.0, dest_h as f32 - 1.0),
        });
        let quad = order_corners(quad);
        if dist(quad[0], quad[1]) <= 3.0 || dist(quad[0], quad[3]) <= 3.0 {
            continue;
        }
        out.push((quad, score));
    }
    out
}

pub(crate) fn dist(a: Pt, b: Pt) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Thresholded map dilated by a 2x2 kernel anchored at its bottom-right
/// cell: a pixel is set when it or its left, upper or upper-left neighbour
/// clears the threshold.
fn dilated_mask(map: &ProbMap) -> Vec<bool> {
    let on = |x: usize, y: usize| map.p[y * map.w + x] > THRESH;
    let mut m = vec![false; map.w * map.h];
    for y in 0..map.h {
        for x in 0..map.w {
            m[y * map.w + x] = on(x, y)
                || (x > 0 && on(x - 1, y))
                || (y > 0 && on(x, y - 1))
                || (x > 0 && y > 0 && on(x - 1, y - 1));
        }
    }
    m
}

/// Edge pixels of each 8-connected region; enough for the convex hull.
fn regions(mask: &[bool], w: usize, h: usize) -> Vec<Vec<Pt>> {
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        seen[start] = true;
        stack.push(start);
        let mut edge = Vec::new();
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let mut interior = true;
            for (dx, dy) in [
                (-1i64, -1i64),
                (0, -1),
                (1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    interior = false;
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if !mask[j] {
                    interior = false;
                    continue;
                }
                if !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
            if !interior {
                edge.push(Pt {
                    x: x as f32,
                    y: y as f32,
                });
            }
        }
        out.push(edge);
    }
    out
}

/// Mean probability over the pixels whose centres fall inside the convex
/// quad (edges included).
fn mean_inside(map: &ProbMap, q: [Pt; 4]) -> f32 {
    let lo_x = q
        .iter()
        .map(|p| p.x)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as usize;
    let hi_x = (q.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil() as usize).min(map.w - 1);
    let lo_y = q
        .iter()
        .map(|p| p.y)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as usize;
    let hi_y = (q.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil() as usize).min(map.h - 1);
    let (mut sum, mut n) = (0f64, 0u32);
    for y in lo_y..=hi_y {
        for x in lo_x..=hi_x {
            if inside(
                q,
                Pt {
                    x: x as f32,
                    y: y as f32,
                },
            ) {
                sum += f64::from(map.p[y * map.w + x]);
                n += 1;
            }
        }
    }
    match n {
        0 => 0.0,
        _ => (sum / f64::from(n)) as f32,
    }
}

fn inside(q: [Pt; 4], p: Pt) -> bool {
    let side = |a: Pt, b: Pt| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let s: Vec<f32> = (0..4).map(|i| side(q[i], q[(i + 1) % 4])).collect();
    s.iter().all(|&v| v >= -0.5) || s.iter().all(|&v| v <= 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_with(w: usize, h: usize, rects: &[(usize, usize, usize, usize)]) -> Vec<f32> {
        let mut p = vec![0.0; w * h];
        for &(x0, y0, x1, y1) in rects {
            for y in y0..y1 {
                for x in x0..x1 {
                    p[y * w + x] = 0.9;
                }
            }
        }
        p
    }

    #[test]
    fn two_text_lines_give_two_grown_boxes() {
        let p = map_with(100, 60, &[(10, 10, 80, 18), (10, 30, 60, 38)]);
        let map = ProbMap {
            w: 100,
            h: 60,
            p: &p,
        };
        let mut b = boxes(&map, 200, 120);
        b.sort_by(|a, b| a.0[0].y.total_cmp(&b.0[0].y));
        assert_eq!(b.len(), 2);
        let first = b[0].0;
        // 70x8 core (plus the dilation) grows by ~5 each side, then doubles
        assert!(first[0].x < 15.0 && first[1].x > 165.0, "{first:?}");
        assert!(first[0].y < 15.0 && first[3].y > 40.0, "{first:?}");
        // the dilated rim around the core reads 0, as it does in the reference
        assert!(b[0].1 > 0.6, "{}", b[0].1);
    }

    #[test]
    fn specks_and_faint_areas_are_dropped() {
        let mut p = map_with(50, 50, &[(5, 5, 7, 7)]);
        for v in p[20 * 50..30 * 50].iter_mut() {
            *v = 0.35; // above the threshold, below the box score
        }
        let map = ProbMap {
            w: 50,
            h: 50,
            p: &p,
        };
        assert!(boxes(&map, 50, 50).is_empty());
    }
}
