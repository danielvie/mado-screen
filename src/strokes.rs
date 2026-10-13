#[derive(Default)]
pub struct Strokes {
    paths: Vec<Vec<(i32, i32)>>,
    active: bool,
}

impl Strokes {
    pub fn begin(&mut self, point: (i32, i32)) {
        self.paths.push(vec![point]);
        self.active = true;
    }

    pub fn append(&mut self, point: (i32, i32)) -> bool {
        if !self.active {
            return false;
        }
        // An active stroke always has the path and initial point created by begin.
        let path = self.paths.last_mut().unwrap();
        if path.last() == Some(&point) {
            return false;
        }
        path.push(point);
        true
    }

    pub fn finish(&mut self) {
        self.active = false;
    }

    pub fn clear(&mut self) {
        self.paths.clear();
        self.finish();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn paths(&self) -> &[Vec<(i32, i32)>] {
        &self.paths
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty_and_inactive() {
        let strokes = Strokes::default();
        assert!(strokes.paths().is_empty());
        assert!(!strokes.is_active());
    }

    #[test]
    fn ignores_hover() {
        let mut strokes = Strokes::default();
        assert!(!strokes.append((12, -4)));
        assert!(strokes.paths().is_empty());
        assert!(!strokes.is_active());
    }

    #[test]
    fn click_keeps_a_dot() {
        let mut strokes = Strokes::default();
        strokes.begin((-12, 4));
        assert!(strokes.is_active());
        strokes.finish();
        assert!(!strokes.is_active());
        assert_eq!(strokes.paths(), &[vec![(-12, 4)]]);
    }

    #[test]
    fn ignores_only_consecutive_duplicate_points() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        assert!(!strokes.append((1, 2)));
        assert!(strokes.append((3, 4)));
        assert!(!strokes.append((3, 4)));
        assert!(strokes.append((1, 2)));
        assert_eq!(strokes.paths(), &[vec![(1, 2), (3, 4), (1, 2)]]);
    }

    #[test]
    fn begin_separates_strokes_even_while_active() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        assert!(strokes.append((3, 4)));
        strokes.begin((5, 6));
        assert!(strokes.append((7, 8)));
        strokes.finish();
        strokes.begin((9, 10));
        assert!(strokes.is_active());
        assert_eq!(
            strokes.paths(),
            &[vec![(1, 2), (3, 4)], vec![(5, 6), (7, 8)], vec![(9, 10)]]
        );
    }

    #[test]
    fn finish_for_mode_change_preserves_ink_and_stops_append() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        assert!(strokes.append((3, 4)));
        strokes.finish();
        strokes.finish();
        assert!(!strokes.is_active());
        assert!(!strokes.append((5, 6)));
        assert_eq!(strokes.paths(), &[vec![(1, 2), (3, 4)]]);
    }

    #[test]
    fn clear_during_active_stroke_stops_append_until_new_begin() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        strokes.finish();
        strokes.begin((3, 4));
        strokes.clear();
        assert!(strokes.paths().is_empty());
        assert!(!strokes.is_active());
        assert!(!strokes.append((5, 6)));
        assert!(strokes.paths().is_empty());
        strokes.begin((7, 8));
        assert!(strokes.append((9, 10)));
        assert_eq!(strokes.paths(), &[vec![(7, 8), (9, 10)]]);
    }
}
