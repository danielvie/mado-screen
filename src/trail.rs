use crate::{
    bezier::{CurvePoint, append_quadratic, midpoint},
    strokes::{Annotation, Point, Style},
};
use std::{collections::VecDeque, time::Duration};

#[derive(Copy, Clone)]
struct Active {
    last: Point,
    style: Style,
    then: Duration,
    start: CurvePoint,
    live: bool,
}

/// Temporary ink with caller-supplied monotonic elapsed times, independent of history.
pub struct Trail {
    lifetime: Duration,
    parts: VecDeque<(Duration, u64, Annotation)>,
    active: Option<Active>,
    gesture: u64,
}

impl Default for Trail {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

impl Trail {
    pub fn new(lifetime: Duration) -> Self {
        assert!(
            !lifetime.is_zero(),
            "Temporary ink lifetime must be positive"
        );
        Self {
            lifetime,
            parts: VecDeque::new(),
            active: None,
            gesture: 0,
        }
    }

    pub fn begin(&mut self, point: Point, style: Style, now: Duration) -> bool {
        let now = self.parts.back().map_or(now, |part| now.max(part.0));
        self.gesture += 1;
        self.parts.push_back((
            now,
            self.gesture,
            Annotation {
                paths: vec![vec![point]],
                style,
            },
        ));
        self.active = Some(Active {
            last: point,
            style,
            then: now,
            start: midpoint(point, point),
            live: false,
        });
        true
    }

    pub fn append(&mut self, point: Point, now: Duration) -> bool {
        let Some(mut active) = self.active else {
            return false;
        };
        if point == active.last {
            return false;
        }
        let now = now.max(active.then);
        let path = if now.saturating_sub(active.then) >= self.lifetime {
            // A pause starts an isolated dot, not a curve from expired ink.
            active.start = midpoint(point, point);
            active.live = false;
            vec![point]
        } else {
            if active.live {
                // Expiry can already have removed the replaceable back part.
                if self
                    .parts
                    .back()
                    .is_some_and(|part| part.0 == active.then && part.1 == self.gesture)
                {
                    self.parts.pop_back();
                }
                let end = midpoint(active.last, point);
                let mut path = vec![(active.start.0.round() as i32, active.start.1.round() as i32)];
                append_quadratic(&mut path, active.start, active.last, end);
                // The finalized curve inherits the replaced tail's birth, never
                // the current sample's time. Older finalized pieces stay untouched.
                self.parts.push_back((
                    active.then,
                    self.gesture,
                    Annotation {
                        paths: vec![path],
                        style: active.style,
                    },
                ));
                active.start = end;
            }
            active.live = true;
            let start = (active.start.0.round() as i32, active.start.1.round() as i32);
            if start == point {
                vec![point]
            } else {
                vec![start, point]
            }
        };
        // Dots and new live tails are born at their input time. Each part expires
        // together after the configured lifetime, including sparse-event curves. ponytail:
        // interpolate births along a piece if per-distance expiry is later needed.
        self.parts.push_back((
            now,
            self.gesture,
            Annotation {
                paths: vec![path],
                style: active.style,
            },
        ));
        active.last = point;
        active.then = now;
        self.active = Some(active);
        true
    }

    pub fn finish(&mut self) {
        self.active = None;
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.is_empty();
        self.parts.clear();
        self.finish();
        changed
    }

    pub fn expire(&mut self, now: Duration) -> bool {
        let mut changed = false;
        while self
            .parts
            .front()
            .is_some_and(|(born, _, _)| now.saturating_sub(*born) >= self.lifetime)
        {
            self.parts.pop_front();
            changed = true;
        }
        changed
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn paint_parts(&self, now: Duration) -> impl Iterator<Item = (u64, &Annotation, u8)> {
        self.parts.iter().map(move |(born, gesture, annotation)| {
            let remaining = self.lifetime.saturating_sub(now.saturating_sub(*born));
            let alpha = (remaining.as_nanos() * 255 + self.lifetime.as_nanos() / 2)
                / self.lifetime.as_nanos();
            (*gesture, annotation, alpha as u8)
        })
    }

    #[cfg(test)]
    pub fn annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.parts.iter().map(|(_, _, annotation)| annotation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_duration_controls_fade_expiry_and_pause_boundary() {
        for lifetime_ms in [1, 250, 2500, 60000] {
            let lifetime = Duration::from_millis(lifetime_ms);
            let mut trail = Trail::new(lifetime);
            trail.begin((0, 0), Style::default(), Duration::ZERO);
            assert_eq!(trail.paint_parts(Duration::ZERO).next().unwrap().2, 255);
            assert_eq!(trail.paint_parts(lifetime / 2).next().unwrap().2, 128);
            assert!(!trail.expire(lifetime - Duration::from_nanos(1)));
            assert_eq!(trail.paint_parts(lifetime).next().unwrap().2, 0);
            assert!(trail.expire(lifetime));
            assert!(trail.is_empty());
            // Movement after a full lifetime starts an isolated dot, even during a drag.
            trail.append((10, 10), lifetime);
            assert_eq!(paths(&trail), [vec![(10, 10)]]);
            trail.finish();
            assert_eq!(trail.paint_parts(lifetime).next().unwrap().2, 255);
            assert!(!trail.expire(lifetime * 2 - Duration::from_nanos(1)));
            assert!(trail.expire(lifetime * 2));
        }
    }

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn paths(trail: &Trail) -> Vec<Vec<Point>> {
        trail
            .annotations()
            .map(|annotation| annotation.paths[0].clone())
            .collect()
    }

    #[test]
    fn poc7_opacity_fades_over_the_full_second_without_refresh_on_release() {
        let mut trail = Trail::default();
        trail.begin((5, 5), Style::default(), ms(0));
        for (age, alpha) in [
            (0, 255),
            (250, 191),
            (500, 128),
            (750, 64),
            (999, 0),
            (1000, 0),
        ] {
            assert_eq!(trail.paint_parts(ms(age)).next().unwrap().2, alpha);
        }
        trail.finish();
        assert_eq!(trail.paint_parts(ms(750)).next().unwrap().2, 64);
        assert!(trail.expire(ms(1000)));
        assert!(trail.paint_parts(ms(0)).next().is_none());
    }

    #[test]
    fn poc7_part_ages_and_gesture_groups_are_independent() {
        let mut trail = Trail::default();
        trail.begin((5, 5), Style::default(), ms(0));
        trail.append((10, 5), ms(250));
        trail.finish();
        trail.begin((5, 5), Style::default(), ms(500));
        let parts = trail.paint_parts(ms(500)).collect::<Vec<_>>();
        assert_eq!(
            parts.iter().map(|p| p.2).collect::<Vec<_>>(),
            [128, 191, 255]
        );
        assert_eq!(parts[0].0, parts[1].0);
        assert_ne!(parts[1].0, parts[2].0);
        assert_eq!(trail.paint_parts(Duration::MAX).next().unwrap().2, 0);
    }

    #[test]
    fn bezier_temporary_live_tail_curve_births_and_finish() {
        let mut trail = Trail::default();
        let style = Style {
            color: 0xff123456,
            thickness: 11,
        };
        trail.begin((0, 0), style, ms(0));
        trail.append((100, 100), ms(100));
        trail.append((200, 0), ms(200));
        assert_eq!(
            trail.parts.iter().map(|part| part.0).collect::<Vec<_>>(),
            [ms(0), ms(100), ms(200)]
        );
        let curve = trail.parts[1].2.clone();
        assert_eq!(curve.paths[0].first(), Some(&(0, 0)));
        assert_eq!(curve.paths[0].last(), Some(&(150, 50)));
        assert!(!curve.paths[0].contains(&(100, 100)));
        assert!(
            curve.paths[0]
                .iter()
                .any(|&(x, y)| x > y && y > 0 && x < 150)
        );
        assert_eq!(trail.parts[2].2.paths[0], [(150, 50), (200, 0)]);
        trail.append((250, 100), ms(300));
        assert_eq!(trail.parts[1].2, curve); // No re-tessellation or age refresh.
        assert_eq!(trail.parts[2].0, ms(200));
        assert_eq!(trail.parts[3].2.paths[0], [(225, 50), (250, 100)]);
        assert!(trail.annotations().all(|part| part.style == style));
        let groups = trail
            .paint_parts(ms(300))
            .map(|part| part.0)
            .collect::<Vec<_>>();
        assert!(groups.iter().all(|group| *group == groups[0]));
        let preview = paths(&trail);
        trail.finish();
        assert_eq!(paths(&trail), preview);
        assert!(!trail.append((300, 0), ms(400)));
        assert!(trail.expire(ms(1100)));
        assert_eq!(
            trail.parts.iter().map(|part| part.0).collect::<Vec<_>>(),
            [ms(200), ms(300)]
        );
        assert!(!trail.expire(ms(1199)));
        assert!(trail.expire(ms(1200)));
        assert_eq!(paths(&trail), [vec![(225, 50), (250, 100)]]);
        assert!(trail.expire(ms(1300)));
        assert!(trail.is_empty());
    }

    #[test]
    fn bezier_temporary_pause_expiry_duplicates_and_isolated_restart() {
        for expire_first in [false, true] {
            let mut trail = Trail::default();
            trail.begin((0, 0), Style::default(), ms(0));
            trail.append((100, 100), ms(100));
            trail.append((200, 0), ms(200));
            let parts = trail.parts.clone();
            assert!(!trail.append((200, 0), ms(1199)));
            assert_eq!(trail.parts, parts);
            if expire_first {
                assert!(trail.expire(ms(1200)));
                assert!(trail.is_empty());
            }
            assert!(!trail.append((200, 0), ms(1200)));
            trail.append((400, 0), ms(1200));
            assert_eq!(trail.parts.back().unwrap().2.paths[0], [(400, 0)]);
            trail.expire(ms(1200));
            assert_eq!(paths(&trail), [vec![(400, 0)]]);
            trail.append((500, 100), ms(1201));
            trail.append((600, 0), ms(1202));
            assert!(
                trail
                    .annotations()
                    .flat_map(|part| &part.paths[0])
                    .all(|point| point.0 >= 400)
            );
            assert_eq!(
                trail.parts.back().unwrap().2.paths[0].last(),
                Some(&(600, 0))
            );
        }
    }

    #[test]
    fn bezier_temporary_independent_gestures_and_monotonic_births() {
        let mut trail = Trail::default();
        for (start, now) in [
            ((0, 0), ms(100)),
            ((100, 100), ms(90)),
            ((200, 200), ms(105)),
        ] {
            trail.begin(start, Style::default(), now);
            trail.append((start.0 + 100, start.1 + 100), now);
            trail.append((start.0 + 200, start.1), now);
        }
        let parts = trail.parts.iter().collect::<Vec<_>>();
        assert!(parts.windows(2).all(|pair| pair[0].0 <= pair[1].0));
        assert_eq!(parts.len(), 9);
        for (index, start) in [(0, 0), (100, 100), (200, 200)].into_iter().enumerate() {
            let group = &parts[index * 3..index * 3 + 3];
            assert_eq!(group[0].2.paths[0], [start]);
            assert_eq!(group[1].2.paths[0].first(), Some(&start));
            assert!(group.iter().all(|part| part.1 == index as u64 + 1));
        }
        assert!(trail.clear());
        assert!(!trail.is_active());
        assert!(!trail.append((999, 999), ms(200)));
    }

    #[test]
    fn bezier_temporary_small_signed_reversals_preserve_press_and_turn() {
        for (start, turn) in [((-1, -1), (0, 0)), ((1, 1), (0, 0))] {
            let mut trail = Trail::default();
            trail.begin(start, Style::default(), ms(0));
            trail.append(turn, ms(1));
            trail.append(start, ms(2));
            assert_eq!(trail.parts[0].2.paths[0], [start]);
            assert!(trail.parts[1].2.paths[0].len() > 1);
            assert_eq!(trail.parts.back().unwrap().2.paths[0].last(), Some(&start));
            let preview = paths(&trail);
            trail.finish();
            assert_eq!(paths(&trail), preview);
        }
    }

    #[test]
    fn defaults_hover_duplicates_and_release() {
        let mut trail = Trail::default();
        assert!(trail.is_empty());
        assert!(!trail.is_active());
        assert_eq!(trail.annotations().count(), 0);
        assert!(!trail.append((1, 2), ms(0)));
        assert!(!trail.expire(ms(1000)));
        assert!(!trail.clear());
        trail.finish();
        assert!(trail.begin((1, 2), Style::default(), ms(0)));
        assert!(trail.is_active());
        assert!(!trail.append((1, 2), ms(500)));
        assert_eq!(paths(&trail), [vec![(1, 2)]]);
        assert!(trail.append((3, 4), ms(501)));
        assert!(!trail.append((3, 4), ms(502)));
        trail.finish();
        trail.finish();
        assert!(!trail.is_active());
        assert!(!trail.append((5, 6), ms(503)));
        assert_eq!(paths(&trail), [vec![(1, 2)], vec![(1, 2), (3, 4)]]);
    }

    #[test]
    fn staggered_parts_expire_at_the_exact_boundary() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        trail.append((1, 0), ms(250));
        trail.append((2, 0), ms(500));
        assert!(!trail.expire(ms(999)));
        assert_eq!(trail.annotations().count(), 3);
        assert!(trail.expire(ms(1000)));
        assert_eq!(paths(&trail), [vec![(0, 0), (2, 0)], vec![(2, 0)]]);
        assert!(!trail.expire(ms(1249)));
        assert!(trail.expire(ms(1250)));
        assert_eq!(paths(&trail), [vec![(2, 0)]]);
        assert!(!trail.expire(ms(1499)));
        assert!(trail.expire(ms(1500)));
        assert!(trail.is_empty());
        assert!(!trail.expire(ms(1500)));
        assert!(!trail.expire(ms(500))); // Removed ink never returns.
    }

    #[test]
    fn expiry_works_during_drag_and_after_release() {
        for released in [false, true] {
            let mut trail = Trail::default();
            trail.begin((0, 0), Style::default(), ms(0));
            trail.append((10, 0), ms(100));
            if released {
                trail.finish();
            }
            assert!(trail.expire(ms(1100)));
            assert!(trail.is_empty());
            assert_eq!(trail.is_active(), !released);
            assert_eq!(trail.append((20, 0), ms(1100)), !released);
            if released {
                assert!(trail.is_empty());
            } else {
                assert_eq!(paths(&trail), [vec![(20, 0)]]);
            }
        }
    }

    #[test]
    fn pause_boundary_never_reconnects_an_expired_anchor() {
        for gap in [999, 1000, 1001] {
            for expire_first in [false, true] {
                let mut trail = Trail::default();
                trail.begin((0, 0), Style::default(), ms(0));
                if expire_first {
                    assert_eq!(trail.expire(ms(gap)), gap >= 1000);
                }
                assert!(trail.append((10, 0), ms(gap)));
                assert_eq!(
                    trail.annotations().last().unwrap().paths,
                    [if gap < 1000 {
                        vec![(0, 0), (10, 0)]
                    } else {
                        vec![(10, 0)]
                    }]
                );
                assert!(trail.append((20, 0), ms(gap + 1)));
                assert_eq!(
                    trail.annotations().last().unwrap().paths,
                    [if gap < 1000 {
                        vec![(15, 0), (20, 0)]
                    } else {
                        vec![(10, 0), (20, 0)]
                    }]
                );
            }
        }
    }

    #[test]
    fn duplicates_do_not_refresh_an_expired_anchor() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        assert!(!trail.append((0, 0), ms(999)));
        assert!(trail.expire(ms(1000)));
        assert!(!trail.append((0, 0), ms(1001)));
        assert!(trail.is_empty());
        assert!(trail.append((10, 0), ms(1002)));
        assert_eq!(paths(&trail), [vec![(10, 0)]]);
    }

    #[test]
    fn independent_gestures_never_bridge_even_when_begin_interrupts() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        trail.append((1, 0), ms(1));
        trail.finish();
        assert!(!trail.append((50, 50), ms(2)));
        trail.begin((100, 0), Style::default(), ms(3));
        trail.append((101, 0), ms(4));
        trail.begin((200, 0), Style::default(), ms(5));
        trail.append((201, 0), ms(6));
        trail.begin((201, 0), Style::default(), ms(7));
        assert_eq!(
            paths(&trail),
            [
                vec![(0, 0)],
                vec![(0, 0), (1, 0)],
                vec![(100, 0)],
                vec![(100, 0), (101, 0)],
                vec![(200, 0)],
                vec![(200, 0), (201, 0)],
                vec![(201, 0)],
            ]
        );
    }

    #[test]
    fn styles_are_frozen_for_segments_and_pause_dots() {
        let mut trail = Trail::default();
        let mut style = Style::default();
        let first_style = style;
        trail.begin((0, 0), style, ms(0));
        style.color = 0xff123456;
        style.thickness = 11;
        trail.append((1, 0), ms(1));
        trail.append((2, 0), ms(1001));
        assert!(trail.annotations().all(|part| part.style == first_style));
        trail.begin((10, 0), style, ms(2002));
        trail.append((11, 0), ms(2003));
        assert_eq!(
            trail
                .annotations()
                .map(|part| part.style)
                .collect::<Vec<_>>(),
            [first_style, first_style, first_style, style, style]
        );
    }

    #[test]
    fn clear_removes_parts_and_finishes_even_an_empty_active_drag() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        trail.append((1, 0), ms(1));
        assert!(trail.clear());
        assert!(trail.is_empty());
        assert!(!trail.is_active());
        assert!(!trail.clear());
        assert!(!trail.append((2, 0), ms(2)));
        assert!(!trail.expire(ms(1001)));
        trail.begin((10, 0), Style::default(), ms(1002));
        assert!(trail.expire(ms(2002)));
        assert!(trail.is_active());
        assert!(!trail.clear());
        assert!(!trail.is_active());
        assert!(!trail.append((11, 0), ms(4003)));
    }

    #[test]
    fn sparse_segments_have_one_birth_time() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        trail.append((1000, 0), ms(999));
        assert!(trail.expire(ms(1000)));
        assert_eq!(paths(&trail), [vec![(0, 0), (1000, 0)]]);
        assert!(!trail.expire(ms(1998)));
        assert!(trail.expire(ms(1999)));
        assert!(trail.is_empty());
    }

    #[test]
    fn time_subtraction_does_not_underflow_or_overflow() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(1000));
        assert!(!trail.expire(ms(999)));
        assert!(trail.append((1, 0), ms(999)));
        assert_eq!(paths(&trail), [vec![(0, 0)], vec![(0, 0), (1, 0)]]);
        assert!(trail.expire(Duration::MAX));
        assert!(trail.is_empty());
        assert!(trail.append((2, 0), Duration::MAX));
        assert_eq!(paths(&trail), [vec![(2, 0)]]);
        assert!(!trail.expire(Duration::MAX));
    }
}
