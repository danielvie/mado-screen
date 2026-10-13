use crate::strokes::Point;

pub type CurvePoint = (f64, f64);

pub fn midpoint(a: Point, b: Point) -> CurvePoint {
    (
        (f64::from(a.0) + f64::from(b.0)) / 2.0,
        (f64::from(a.1) + f64::from(b.1)) / 2.0,
    )
}

pub fn append_quadratic(path: &mut Vec<Point>, start: CurvePoint, control: Point, end: CurvePoint) {
    fn flatten(path: &mut Vec<Point>, a: CurvePoint, b: CurvePoint, c: CurvePoint, depth: u8) {
        let (dx, dy) = (c.0 - a.0, c.1 - a.1);
        let (px, py) = (b.0 - a.0, b.1 - a.1);
        let length_squared = dx * dx + dy * dy;
        let t = if length_squared == 0.0 {
            0.0
        } else {
            (px * dx + py * dy) / length_squared
        };
        let nearest = t.clamp(0.0, 1.0);
        // Control-point distance to the chord bounds curve flatness to 0.5px.
        // Integer rounding adds at most sqrt(0.5)px. ponytail: depth 12 caps a
        // segment at 4096 edges; extreme i32 curves can exceed the flatness budget.
        // Split reversals even when flat so rounding cannot erase their turn.
        if depth == 12
            || ((0.0..=1.0).contains(&t)
                && (px - nearest * dx).powi(2) + (py - nearest * dy).powi(2) <= 0.25)
        {
            let point = (c.0.round() as i32, c.1.round() as i32);
            if path.last() != Some(&point) {
                path.push(point);
            }
            return;
        }
        let ab = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let bc = ((b.0 + c.0) / 2.0, (b.1 + c.1) / 2.0);
        let mid = ((ab.0 + bc.0) / 2.0, (ab.1 + bc.1) / 2.0);
        flatten(path, a, ab, mid, depth + 1);
        flatten(path, mid, bc, c, depth + 1);
    }
    flatten(
        path,
        start,
        (f64::from(control.0), f64::from(control.1)),
        end,
        0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_midpoints_and_extremes_are_bounded_without_overflow() {
        assert_eq!(
            midpoint((i32::MIN, i32::MAX), (i32::MAX, i32::MIN)),
            (-0.5, -0.5)
        );
        for (start, control, end) in [
            (
                (i32::MIN, i32::MIN),
                (i32::MAX, i32::MAX),
                (i32::MIN, i32::MAX),
            ),
            (
                (i32::MAX, i32::MIN),
                (i32::MIN, i32::MAX),
                (i32::MAX, i32::MIN),
            ),
            ((0, 0), (0, 0), (0, 0)),
            ((0, 0), (100, 0), (0, 0)),
        ] {
            let mut path = vec![start];
            append_quadratic(
                &mut path,
                midpoint(start, start),
                control,
                midpoint(end, end),
            );
            assert_eq!(path.first(), Some(&start));
            assert_eq!(path.last(), Some(&end));
            assert!(path.len() <= 4097);
            assert!(path.windows(2).all(|pair| pair[0] != pair[1]));
            assert!(
                path.iter()
                    .all(|&(x, y)| x >= start.0.min(control.0).min(end.0)
                        && x <= start.0.max(control.0).max(end.0)
                        && y >= start.1.min(control.1).min(end.1)
                        && y <= start.1.max(control.1).max(end.1))
            );
            if start == (0, 0) && control == (100, 0) {
                assert!(path.contains(&(50, 0))); // A reversal must not collapse to a dot.
            }
        }
    }

    #[test]
    fn bezier_bows_and_respects_flatness_plus_integer_rounding() {
        let mut path = vec![(0, 0)];
        append_quadratic(&mut path, (0.0, 0.0), (100, 100), (200.0, 0.0));
        assert!(path.contains(&(100, 50)));
        assert!(!path.contains(&(100, 100)));
        for sample in 0..=1000 {
            let t = sample as f64 / 1000.0;
            let (x, y) = (200.0 * t, 200.0 * t * (1.0 - t));
            let distance = path
                .windows(2)
                .map(|pair| {
                    let (dx, dy) = (
                        (pair[1].0 - pair[0].0) as f64,
                        (pair[1].1 - pair[0].1) as f64,
                    );
                    let (px, py) = (x - pair[0].0 as f64, y - pair[0].1 as f64);
                    let u = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                    (px - u * dx).hypot(py - u * dy)
                })
                .fold(f64::INFINITY, f64::min);
            assert!(distance <= 0.5 + 0.5_f64.sqrt());
        }
    }
}
