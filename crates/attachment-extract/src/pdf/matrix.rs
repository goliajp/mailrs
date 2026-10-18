//! 2d affine matrices in pdf's `[a b c d e f]` row-vector form

pub(super) type M = [f64; 6];

pub(super) const IDENTITY: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a × b`: apply `a` first, then `b`.
pub(super) fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// `translate(tx, ty) × m`
pub(super) fn translate(m: &M, tx: f64, ty: f64) -> M {
    [
        m[0],
        m[1],
        m[2],
        m[3],
        tx * m[0] + ty * m[2] + m[4],
        tx * m[1] + ty * m[3] + m[5],
    ]
}

pub(super) fn apply(m: &M, x: f64, y: f64) -> (f64, f64) {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}

/// Unit device-space direction of the text-space vector `v`; `(1, 0)` when degenerate.
pub(super) fn dir(m: &M, v: (f64, f64)) -> (f64, f64) {
    let (x, y) = (v.0 * m[0] + v.1 * m[2], v.0 * m[1] + v.1 * m[3]);
    let len = x.hypot(y);
    if len > 0.0 && len.is_finite() {
        (x / len, y / len)
    } else {
        (1.0, 0.0)
    }
}

/// Linear scale factor of `m` (square root of the area scale).
pub(super) fn scale(m: &M) -> f64 {
    (m[0] * m[3] - m[1] * m[2]).abs().sqrt()
}
