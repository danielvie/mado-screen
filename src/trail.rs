use crate::strokes::{Annotation, Point, Style};
use std::{collections::VecDeque, time::Duration};

const LIFETIME: Duration = Duration::from_secs(2);

/// Temporary ink with caller-supplied monotonic elapsed times, independent of history.
#[derive(Default)]
pub struct Trail {
    parts: VecDeque<(Duration, Annotation)>,
    active: Option<(Point, Style, Duration)>,
}

impl Trail {
    pub fn begin(&mut self, point: Point, style: Style, now: Duration) -> bool {
        self.parts.push_back((
            now,
            Annotation {
                paths: vec![vec![point]],
                style,
            },
        ));
        self.active = Some((point, style, now));
        true
    }

    pub fn append(&mut self, point: Point, now: Duration) -> bool {
        let Some((last, style, then)) = self.active else {
            return false;
        };
        if point == last {
            return false;
        }
        let path = if now.saturating_sub(then) >= LIFETIME {
            vec![point]
        } else {
            vec![last, point]
        };
        // ponytail: each input segment expires together, even with sparse events;
        // subdivide and interpolate birth times if per-distance expiry is needed.
        self.parts.push_back((
            now,
            Annotation {
                paths: vec![path],
                style,
            },
        ));
        self.active = Some((point, style, now));
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
            .is_some_and(|(born, _)| now.saturating_sub(*born) >= LIFETIME)
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

    pub fn annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.parts.iter().map(|(_, annotation)| annotation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn defaults_hover_duplicates_and_release() {
        let mut trail = Trail::default();
        assert!(trail.is_empty());
        assert!(!trail.is_active());
        assert_eq!(trail.annotations().count(), 0);
        assert!(!trail.append((1, 2), ms(0)));
        assert!(!trail.expire(ms(2000)));
        assert!(!trail.clear());
        trail.finish();
        assert!(trail.begin((1, 2), Style::default(), ms(0)));
        assert!(trail.is_active());
        assert!(!trail.append((1, 2), ms(1000)));
        assert_eq!(paths(&trail), [vec![(1, 2)]]);
        assert!(trail.append((3, 4), ms(1001)));
        assert!(!trail.append((3, 4), ms(1002)));
        trail.finish();
        trail.finish();
        assert!(!trail.is_active());
        assert!(!trail.append((5, 6), ms(1003)));
        assert_eq!(paths(&trail), [vec![(1, 2)], vec![(1, 2), (3, 4)]]);
    }

    #[test]
    fn staggered_parts_expire_at_the_exact_boundary() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        trail.append((1, 0), ms(500));
        trail.append((2, 0), ms(1000));
        assert!(!trail.expire(ms(1999)));
        assert_eq!(trail.annotations().count(), 3);
        assert!(trail.expire(ms(2000)));
        assert_eq!(paths(&trail), [vec![(0, 0), (1, 0)], vec![(1, 0), (2, 0)]]);
        assert!(!trail.expire(ms(2499)));
        assert!(trail.expire(ms(2500)));
        assert_eq!(paths(&trail), [vec![(1, 0), (2, 0)]]);
        assert!(!trail.expire(ms(2999)));
        assert!(trail.expire(ms(3000)));
        assert!(trail.is_empty());
        assert!(!trail.expire(ms(3000)));
        assert!(!trail.expire(ms(1000))); // Removed ink never returns.
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
            assert!(trail.expire(ms(2100)));
            assert!(trail.is_empty());
            assert_eq!(trail.is_active(), !released);
            assert_eq!(trail.append((20, 0), ms(2100)), !released);
            if released {
                assert!(trail.is_empty());
            } else {
                assert_eq!(paths(&trail), [vec![(20, 0)]]);
            }
        }
    }

    #[test]
    fn pause_boundary_never_reconnects_an_expired_anchor() {
        for gap in [1999, 2000, 2001] {
            for expire_first in [false, true] {
                let mut trail = Trail::default();
                trail.begin((0, 0), Style::default(), ms(0));
                if expire_first {
                    assert_eq!(trail.expire(ms(gap)), gap >= 2000);
                }
                assert!(trail.append((10, 0), ms(gap)));
                assert_eq!(
                    trail.annotations().last().unwrap().paths,
                    [if gap < 2000 {
                        vec![(0, 0), (10, 0)]
                    } else {
                        vec![(10, 0)]
                    }]
                );
                assert!(trail.append((20, 0), ms(gap + 1)));
                assert_eq!(
                    trail.annotations().last().unwrap().paths,
                    [vec![(10, 0), (20, 0)]]
                );
            }
        }
    }

    #[test]
    fn duplicates_do_not_refresh_an_expired_anchor() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        assert!(!trail.append((0, 0), ms(1999)));
        assert!(trail.expire(ms(2000)));
        assert!(!trail.append((0, 0), ms(2001)));
        assert!(trail.is_empty());
        assert!(trail.append((10, 0), ms(2002)));
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
        trail.append((2, 0), ms(2001));
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
        assert!(!trail.expire(ms(2001)));
        trail.begin((10, 0), Style::default(), ms(2002));
        assert!(trail.expire(ms(4002)));
        assert!(trail.is_active());
        assert!(!trail.clear());
        assert!(!trail.is_active());
        assert!(!trail.append((11, 0), ms(4003)));
    }

    #[test]
    fn sparse_segments_have_one_birth_time() {
        let mut trail = Trail::default();
        trail.begin((0, 0), Style::default(), ms(0));
        trail.append((1000, 0), ms(1999));
        assert!(trail.expire(ms(2000)));
        assert_eq!(paths(&trail), [vec![(0, 0), (1000, 0)]]);
        assert!(!trail.expire(ms(3998)));
        assert!(trail.expire(ms(3999)));
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
