#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pen,
    Line,
    Rectangle,
}

#[derive(Default)]
pub struct Strokes {
    paths: Vec<Vec<(i32, i32)>>,
    active: bool,
    tool: Tool,
}

impl Strokes {
    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn select(&mut self, tool: Tool) -> bool {
        if self.tool == tool {
            return false;
        }
        let changed = self.cancel();
        self.tool = tool;
        changed
    }

    pub fn begin(&mut self, point: (i32, i32)) {
        self.cancel();
        self.paths.push(match self.tool {
            Tool::Pen => vec![point],
            Tool::Line => vec![point; 2],
            Tool::Rectangle => vec![point; 5],
        });
        self.active = true;
    }

    pub fn append(&mut self, point: (i32, i32)) -> bool {
        if !self.active {
            return false;
        }
        // An active stroke always has the path and initial point created by begin.
        let path = self.paths.last_mut().unwrap();
        match self.tool {
            Tool::Pen => {
                if path.last() == Some(&point) {
                    return false;
                }
                path.push(point);
            }
            Tool::Line => {
                if path[1] == point {
                    return false;
                }
                path[1] = point;
            }
            Tool::Rectangle => {
                if path[2] == point {
                    return false;
                }
                let anchor = path[0];
                path[1] = (point.0, anchor.1);
                path[2] = point;
                path[3] = (anchor.0, point.1);
            }
        }
        true
    }

    pub fn finish(&mut self) {
        self.active = false;
    }

    pub fn cancel(&mut self) -> bool {
        let changed = self.active && self.tool != Tool::Pen;
        if changed {
            self.paths.pop();
        }
        self.finish();
        changed
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
        assert_eq!(Tool::default(), Tool::Pen);
        assert_eq!(strokes.tool(), Tool::Pen);
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
    fn line_preview_replaces_endpoint_without_trails() {
        let mut strokes = Strokes::default();
        assert!(!strokes.select(Tool::Line));
        strokes.begin((-1, 2));
        assert_eq!(strokes.paths(), &[vec![(-1, 2); 2]]);
        assert!(!strokes.append((-1, 2)));
        assert!(strokes.append((3, 4)));
        assert!(strokes.append((5, -6)));
        assert!(!strokes.append((5, -6)));
        assert_eq!(strokes.paths(), &[vec![(-1, 2), (5, -6)]]);
        assert!(strokes.append((-1, 2)));
        assert_eq!(strokes.paths(), &[vec![(-1, 2); 2]]);
    }

    #[test]
    fn rectangle_preview_preserves_corners_in_all_directions_and_degeneracies() {
        let mut strokes = Strokes::default();
        strokes.select(Tool::Rectangle);
        let anchor = (1, 2);
        strokes.begin(anchor);
        assert_eq!(strokes.paths(), &[vec![anchor; 5]]);
        assert!(!strokes.append(anchor));
        for opposite in [(5, 6), (-3, 6), (-3, -4), (5, -4), (1, 6), (5, 2), anchor] {
            assert!(strokes.append(opposite));
            assert!(!strokes.append(opposite));
            assert_eq!(
                strokes.paths(),
                &[vec![
                    anchor,
                    (opposite.0, anchor.1),
                    opposite,
                    (anchor.0, opposite.1),
                    anchor,
                ]]
            );
        }
    }

    #[test]
    fn finish_commits_separate_annotations_and_cancel_only_removes_preview() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        strokes.append((3, 4));
        strokes.finish();
        strokes.select(Tool::Line);
        strokes.begin((5, 6));
        strokes.append((7, 8));
        strokes.finish();
        strokes.finish();
        assert!(!strokes.is_active());
        assert!(!strokes.cancel());
        assert!(!strokes.append((9, 10)));
        strokes.select(Tool::Rectangle);
        strokes.begin((9, 10));
        strokes.append((11, 12));
        strokes.finish();
        let committed = strokes.paths().to_vec();
        assert_eq!(
            committed,
            vec![
                vec![(1, 2), (3, 4)],
                vec![(5, 6), (7, 8)],
                vec![(9, 10), (11, 10), (11, 12), (9, 12), (9, 10)],
            ]
        );
        for tool in [Tool::Line, Tool::Rectangle] {
            assert!(!strokes.select(tool));
            strokes.begin((13, 14));
            strokes.append((15, 16));
            assert!(strokes.cancel());
            assert!(!strokes.is_active());
            assert!(!strokes.append((17, 18)));
            assert!(!strokes.cancel());
            assert_eq!(strokes.paths(), committed);
            strokes.begin((13, 14));
            assert!(strokes.cancel());
            assert_eq!(strokes.paths(), committed);
        }
    }

    #[test]
    fn same_tool_selection_preserves_active_gesture() {
        for tool in [Tool::Pen, Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((1, 2));
            strokes.append((3, 4));
            let preview = strokes.paths().to_vec();
            assert!(!strokes.select(tool));
            assert!(strokes.is_active());
            assert_eq!(strokes.paths(), preview);
            assert!(strokes.append((5, 6)));
        }
    }

    #[test]
    fn changing_tool_cancels_shape_but_preserves_partial_pen() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        strokes.append((3, 4));
        assert!(!strokes.select(Tool::Line));
        assert!(!strokes.is_active());
        let pen = strokes.paths().to_vec();
        for tool in [Tool::Rectangle, Tool::Pen] {
            strokes.begin((5, 6));
            strokes.append((7, 8));
            assert!(strokes.select(tool));
            assert_eq!(strokes.tool(), tool);
            assert!(!strokes.is_active());
            assert_eq!(strokes.paths(), pen);
        }
    }

    #[test]
    fn cancel_preserves_partial_pen_and_ends_gesture() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        strokes.append((3, 4));
        assert!(!strokes.cancel());
        assert!(!strokes.is_active());
        assert!(!strokes.append((5, 6)));
        assert!(!strokes.cancel());
        assert_eq!(strokes.paths(), &[vec![(1, 2), (3, 4)]]);
    }

    #[test]
    fn begin_discards_unfinished_shape_before_new_preview() {
        for tool in [Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.select(tool);
            strokes.begin((1, 2));
            strokes.append((3, 4));
            strokes.finish();
            let committed = strokes.paths()[0].clone();
            strokes.begin((5, 6));
            strokes.append((7, 8));
            strokes.begin((9, 10));
            assert!(strokes.is_active());
            assert_eq!(strokes.paths().len(), 2);
            assert_eq!(strokes.paths()[0], committed);
            assert!(strokes.paths()[1].iter().all(|point| *point == (9, 10)));
            assert!(strokes.append((11, 12)));
        }
    }

    #[test]
    fn clear_during_shape_retains_tool_and_stops_append_until_new_begin() {
        for tool in [Tool::Line, Tool::Rectangle] {
            let mut strokes = Strokes::default();
            strokes.begin((1, 2));
            strokes.finish();
            strokes.select(tool);
            strokes.begin((3, 4));
            strokes.append((5, 6));
            strokes.clear();
            assert_eq!(strokes.tool(), tool);
            assert!(strokes.paths().is_empty());
            assert!(!strokes.is_active());
            assert!(!strokes.append((7, 8)));
            assert!(!strokes.cancel());
            strokes.begin((9, 10));
            assert!(strokes.append((11, 12)));
            assert_eq!(strokes.paths().len(), 1);
            strokes.clear();
            strokes.clear();
            assert_eq!(strokes.tool(), tool);
            assert!(strokes.paths().is_empty());
        }
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
