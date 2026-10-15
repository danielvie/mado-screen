use crate::bezier::{CurvePoint, append_quadratic, midpoint};

pub type Point = (i32, i32);

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pen,
    Line,
    Rectangle,
    Arrow,
    Ellipse,
    Eraser,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Style {
    pub color: u32,
    pub thickness: u32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: 0xfff97316,
            thickness: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub paths: Vec<Vec<Point>>,
    pub style: Style,
}

pub struct Strokes {
    annotations: Vec<Annotation>,
    tool: Tool,
    previous_tool: Option<(Tool, bool)>,
    temporary_pen: bool,
    arrow_tail_first: bool,
    arrow_head_scale: f64,
    style: Style,
    before: Option<Vec<Annotation>>,
    anchor: Point,
    last: Point,
    pen_tail: usize,
    pen_start: CurvePoint,
    // ponytail: full snapshots cost O(total scene points) memory per history action;
    // replace with insertion/removal deltas if long sessions exceed the PoC memory budget.
    undo: Vec<Vec<Annotation>>,
    redo: Vec<Vec<Annotation>>,
}

impl Default for Strokes {
    fn default() -> Self {
        Self {
            annotations: Vec::new(),
            tool: Tool::default(),
            previous_tool: None,
            temporary_pen: false,
            arrow_tail_first: false,
            arrow_head_scale: 1.0,
            style: Style::default(),
            before: None,
            anchor: (0, 0),
            last: (0, 0),
            pen_tail: 0,
            pen_start: (0.0, 0.0),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl Strokes {
    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn temporary_pen(&self) -> bool {
        self.temporary_pen
    }

    pub fn arrow_tail_first(&self) -> bool {
        self.arrow_tail_first
    }

    #[cfg(test)]
    pub fn arrow_head_scale(&self) -> f64 {
        self.arrow_head_scale
    }

    pub fn set_arrow_head_scale(&mut self, scale: f64) -> bool {
        if !scale.is_finite() || !(0.1..=4.0).contains(&scale) || scale == self.arrow_head_scale {
            return false;
        }
        self.cancel();
        self.arrow_head_scale = scale;
        true
    }

    pub fn toggle_arrow_direction(&mut self) -> bool {
        if self.tool == Tool::Arrow {
            return self.select_variant(Tool::Arrow, !self.arrow_tail_first);
        }
        let changed = self.cancel();
        self.arrow_tail_first = !self.arrow_tail_first;
        changed
    }

    pub fn tool_label(&self) -> &'static str {
        match self.tool {
            Tool::Pen if self.temporary_pen => "Temporary pen",
            Tool::Pen => "Pen",
            Tool::Arrow if self.arrow_tail_first() => "Arrow tail-first",
            Tool::Arrow => "Arrow head-first",
            Tool::Line => "Line",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Eraser => "Eraser",
        }
    }

    fn selection(&self) -> (Tool, bool) {
        let variant = match self.tool {
            Tool::Pen => self.temporary_pen,
            Tool::Arrow => self.arrow_tail_first,
            _ => false,
        };
        (self.tool, variant)
    }

    fn select_variant(&mut self, tool: Tool, variant: bool) -> bool {
        if self.selection() == (tool, variant) {
            return false;
        }
        let changed = self.cancel();
        self.previous_tool = Some(self.selection());
        self.tool = tool;
        match tool {
            Tool::Pen => self.temporary_pen = variant,
            Tool::Arrow => self.arrow_tail_first = variant,
            _ => {}
        }
        changed
    }

    pub fn select(&mut self, tool: Tool) -> bool {
        let variant = match tool {
            Tool::Pen => self.temporary_pen,
            Tool::Arrow => self.arrow_tail_first,
            _ => false,
        };
        self.select_variant(tool, variant)
    }

    pub fn select_pen(&mut self, temporary: bool) -> bool {
        self.select_variant(Tool::Pen, temporary)
    }

    pub fn has_previous_tool(&self) -> bool {
        self.previous_tool.is_some()
    }

    pub fn swap_previous_tool(&mut self) -> bool {
        let Some((tool, variant)) = self.previous_tool else {
            return false;
        };
        self.select_variant(tool, variant)
    }

    pub fn style(&self) -> Style {
        self.style
    }

    pub fn set_color(&mut self, color: u32) -> bool {
        let color = color | 0xff000000;
        if self.style.color == color {
            return false;
        }
        self.style.color = color;
        true
    }

    pub fn set_thickness(&mut self, thickness: u32) -> bool {
        let thickness = thickness.clamp(1, 31) | 1;
        if self.style.thickness == thickness {
            return false;
        }
        self.style.thickness = thickness;
        true
    }

    pub fn begin(&mut self, point: Point) -> bool {
        let previous = self.annotations.clone();
        self.cancel();
        self.before = Some(self.annotations.clone());
        self.anchor = point;
        self.last = point;
        self.pen_tail = 0;
        self.pen_start = midpoint(point, point);
        if self.tool == Tool::Eraser {
            self.erase(point, point);
        } else {
            let (start, end) = if self.tool == Tool::Arrow && self.arrow_tail_first {
                (point, self.anchor)
            } else {
                (self.anchor, point)
            };
            self.annotations.push(Annotation {
                paths: shape_with_head_scale(
                    self.tool,
                    start,
                    end,
                    self.style.thickness,
                    self.arrow_head_scale,
                ),
                style: self.style,
            });
        }
        previous != self.annotations
    }

    pub fn append(&mut self, point: Point) -> bool {
        if !self.is_active() || point == self.last {
            return false;
        }
        let previous = self.last;
        self.last = point;
        if self.tool == Tool::Eraser {
            return self.erase(previous, point);
        }
        // Non-eraser gestures always own the last annotation created by begin.
        let annotation = self.annotations.last_mut().unwrap();
        if self.tool == Tool::Pen {
            let path = &mut annotation.paths[0];
            // Keep a two-sample gesture as a straight line. Subsequent samples
            // finalize only the previous tail, never re-tessellate older ink.
            if self.pen_tail == 0 {
                path.push(point);
                self.pen_tail = 1;
                return true;
            }
            path.truncate(self.pen_tail);
            let end = midpoint(previous, point);
            append_quadratic(path, self.pen_start, previous, end);
            self.pen_start = end;
            self.pen_tail = path.len();
            if path.last() != Some(&point) {
                path.push(point);
            }
            return true;
        }
        let (start, end) = if self.tool == Tool::Arrow && self.arrow_tail_first {
            (point, self.anchor)
        } else {
            (self.anchor, point)
        };
        let paths = shape_with_head_scale(
            self.tool,
            start,
            end,
            annotation.style.thickness,
            self.arrow_head_scale,
        );
        if paths == annotation.paths {
            return false;
        }
        annotation.paths = paths;
        true
    }

    pub fn finish(&mut self) {
        if let Some(before) = self.before.take()
            && before != self.annotations
        {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    pub fn cancel(&mut self) -> bool {
        if !self.is_active() {
            return false;
        }
        if matches!(self.tool, Tool::Pen | Tool::Eraser) {
            self.finish();
            return false;
        }
        self.annotations = self.before.take().unwrap();
        true
    }

    pub fn clear(&mut self) -> bool {
        let had_annotations = !self.annotations.is_empty();
        self.cancel();
        if !self.annotations.is_empty() {
            self.undo.push(std::mem::take(&mut self.annotations));
            self.redo.clear();
        }
        had_annotations
    }

    pub fn undo(&mut self) -> bool {
        let previous = self.annotations.clone();
        self.cancel();
        if let Some(scene) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.annotations, scene));
        }
        previous != self.annotations
    }

    pub fn redo(&mut self) -> bool {
        let previous = self.annotations.clone();
        self.cancel();
        if let Some(scene) = self.redo.pop() {
            self.undo
                .push(std::mem::replace(&mut self.annotations, scene));
        }
        previous != self.annotations
    }

    pub fn is_active(&self) -> bool {
        self.before.is_some()
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    fn erase(&mut self, start: Point, end: Point) -> bool {
        // Clicks remove only the topmost hit. Moving sweeps remove all hits top to bottom,
        // so overlapping moving sweeps can remove more than one layer.
        let mut changed = false;
        for index in (0..self.annotations.len()).rev() {
            let annotation = &self.annotations[index];
            // 4px hit slop surrounds the painted stroke, not filled shape interiors.
            let radius = annotation.style.thickness as f64 / 2.0 + 4.0;
            let hit = annotation.paths.iter().any(|path| {
                path.first().is_some_and(|&point| {
                    segment_distance_squared(start, end, point, point) <= radius * radius
                }) || path.windows(2).any(|segment| {
                    segment_distance_squared(start, end, segment[0], segment[1]) <= radius * radius
                })
            });
            if hit {
                self.annotations.remove(index);
                changed = true;
                if start == end {
                    break;
                }
            }
        }
        changed
    }
}

#[cfg(test)]
fn shape(tool: Tool, start: Point, end: Point, thickness: u32) -> Vec<Vec<Point>> {
    shape_with_head_scale(tool, start, end, thickness, 1.0)
}

fn shape_with_head_scale(
    tool: Tool,
    start: Point,
    end: Point,
    thickness: u32,
    head_scale: f64,
) -> Vec<Vec<Point>> {
    match tool {
        Tool::Pen => vec![vec![start]],
        Tool::Line => vec![vec![start, end]],
        Tool::Rectangle => vec![vec![start, (end.0, start.1), end, (start.0, end.1), start]],
        Tool::Arrow => {
            let dx = end.0 as f64 - start.0 as f64;
            let dy = end.1 as f64 - start.1 as f64;
            let length = dx.hypot(dy);
            if length == 0.0 {
                return vec![vec![start, end]; 3];
            }
            // The press anchors the head; the moving endpoint is the tail.
            // Larger/thicker heads keep the V visible beyond the shaft's brush width.
            // ponytail: cap at 75% of the arrow length; extremely short thick gestures
            // can still look like a dot. A filled/tapered renderer is the upgrade path.
            let head = ((48.0 + 6.0 * f64::from(thickness.clamp(1, 31) | 1)) * head_scale)
                .min(length * 0.75);
            let (ux, uy) = (dx / length, dy / length);
            let branch = |side: f64| {
                (
                    (start.0 as f64 + ux * head - side * uy * head / 2.0).round() as i32,
                    (start.1 as f64 + uy * head + side * ux * head / 2.0).round() as i32,
                )
            };
            vec![
                vec![start, end],
                vec![start, branch(1.0)],
                vec![start, branch(-1.0)],
            ]
        }
        Tool::Ellipse => {
            let (left, right) = (start.0.min(end.0), start.0.max(end.0));
            let (top, bottom) = (start.1.min(end.1), start.1.max(end.1));
            if left == right || top == bottom {
                return vec![vec![start, end, start]];
            }
            let (rx, ry) = (
                (right as f64 - left as f64) / 2.0,
                (bottom as f64 - top as f64) / 2.0,
            );
            let (cx, cy) = (left as f64 + rx, top as f64 + ry);
            // Bound sagitta to 0.25px at the largest radius, before integer rounding.
            // ponytail: 2048 segments cap work at extreme sizes, where error can exceed
            // the budget; raise the ceiling or use adaptive tessellation if needed.
            let max_angle = 2.0 * (1.0 - 0.25 / rx.max(ry)).acos();
            let steps = ((std::f64::consts::TAU / max_angle).ceil() as usize)
                .clamp(12, 2048)
                .next_multiple_of(4);
            let mut path = Vec::with_capacity(steps + 1);
            for step in 0..steps {
                let angle = std::f64::consts::TAU * step as f64 / steps as f64;
                path.push((
                    ((cx + rx * angle.cos()).round() as i32).clamp(left, right),
                    ((cy + ry * angle.sin()).round() as i32).clamp(top, bottom),
                ));
            }
            path.push(path[0]);
            vec![path]
        }
        Tool::Eraser => unreachable!("eraser does not create paths"),
    }
}

fn point_distance_squared(point: Point, start: Point, end: Point) -> f64 {
    let (dx, dy) = (end.0 as f64 - start.0 as f64, end.1 as f64 - start.1 as f64);
    let (px, py) = (
        point.0 as f64 - start.0 as f64,
        point.1 as f64 - start.1 as f64,
    );
    let length_squared = dx * dx + dy * dy;
    let t = if length_squared == 0.0 {
        0.0
    } else {
        ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
    };
    (px - t * dx).powi(2) + (py - t * dy).powi(2)
}

fn segment_distance_squared(a: Point, b: Point, c: Point, d: Point) -> f64 {
    // i128 cross products keep orientation exact across the full i32 coordinate range.
    let cross = |p: Point, q: Point, r: Point| {
        (q.0 as i128 - p.0 as i128) * (r.1 as i128 - p.1 as i128)
            - (q.1 as i128 - p.1 as i128) * (r.0 as i128 - p.0 as i128)
    };
    let opposite = |x: i128, y: i128| x == 0 || y == 0 || (x < 0) != (y < 0);
    let bounds_overlap = a.0.min(b.0) <= c.0.max(d.0)
        && c.0.min(d.0) <= a.0.max(b.0)
        && a.1.min(b.1) <= c.1.max(d.1)
        && c.1.min(d.1) <= a.1.max(b.1);
    if bounds_overlap
        && opposite(cross(a, b, c), cross(a, b, d))
        && opposite(cross(c, d, a), cross(c, d, b))
    {
        return 0.0;
    }
    point_distance_squared(a, c, d)
        .min(point_distance_squared(b, c, d))
        .min(point_distance_squared(c, a, b))
        .min(point_distance_squared(d, a, b))
}

// Test setup for variant/history regressions. Runtime pen selection is exact.
#[cfg(test)]
impl Strokes {
    pub fn toggle_temporary_pen(&mut self) -> bool {
        if self.tool == Tool::Pen {
            return self.select_pen(!self.temporary_pen);
        }
        let changed = self.cancel();
        self.temporary_pen = !self.temporary_pen;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(strokes: &mut Strokes, tool: Tool, start: Point, end: Point) {
        strokes.select(tool);
        assert!(strokes.begin(start));
        strokes.append(end);
        strokes.finish();
    }

    #[test]
    fn defaults_hover_and_no_ops() {
        let mut strokes = Strokes::default();
        assert_eq!(strokes.tool(), Tool::Pen);
        assert_eq!(strokes.tool_label(), "Pen");
        assert!(!strokes.temporary_pen());
        assert!(!strokes.arrow_tail_first());
        assert_eq!(
            strokes.style(),
            Style {
                color: 0xfff97316,
                thickness: 5
            }
        );
        assert!(strokes.annotations().is_empty());
        assert!(!strokes.is_active());
        assert!(!strokes.append((1, 2)));
        assert!(!strokes.cancel());
        assert!(!strokes.clear());
        assert!(!strokes.undo());
        assert!(!strokes.redo());
        assert!(!strokes.select(Tool::Pen));
        assert!(!strokes.set_color(0x00f97316));
        assert!(!strokes.set_thickness(4));
    }

    #[test]
    fn variant_labels_and_choices_persist() {
        let mut strokes = Strokes::default();
        for (tool, label) in [
            (Tool::Pen, "Pen"),
            (Tool::Line, "Line"),
            (Tool::Rectangle, "Rectangle"),
            (Tool::Arrow, "Arrow head-first"),
            (Tool::Ellipse, "Ellipse"),
            (Tool::Eraser, "Eraser"),
        ] {
            assert!(!strokes.select(tool));
            assert_eq!(strokes.tool_label(), label);
        }
        assert!(!strokes.toggle_temporary_pen());
        assert!(!strokes.toggle_arrow_direction());
        for (tool, label) in [
            (Tool::Pen, "Temporary pen"),
            (Tool::Line, "Line"),
            (Tool::Rectangle, "Rectangle"),
            (Tool::Arrow, "Arrow tail-first"),
            (Tool::Ellipse, "Ellipse"),
            (Tool::Eraser, "Eraser"),
        ] {
            assert!(!strokes.select(tool));
            assert!(!strokes.select(tool));
            assert_eq!(strokes.tool_label(), label);
            assert!(strokes.temporary_pen());
            assert!(strokes.arrow_tail_first());
        }
        draw(&mut strokes, Tool::Line, (0, 0), (60, 30));
        assert!(strokes.clear());
        assert!(!strokes.cancel()); // Drawing/click-through transitions use cancel.
        assert!(strokes.undo());
        assert!(strokes.redo());
        assert!(strokes.temporary_pen());
        assert!(strokes.arrow_tail_first());
        strokes.select(Tool::Pen);
        assert_eq!(strokes.tool_label(), "Temporary pen");
        assert!(!strokes.toggle_temporary_pen());
        assert_eq!(strokes.tool_label(), "Pen");
        strokes.select(Tool::Arrow);
        assert_eq!(strokes.tool_label(), "Arrow tail-first");
        assert!(!strokes.toggle_arrow_direction());
        assert_eq!(strokes.tool_label(), "Arrow head-first");
    }

    #[test]
    fn variant_toggles_preserve_style_scene_and_history_when_idle() {
        let mut strokes = Strokes::default();
        strokes.set_color(0xff123456);
        strokes.set_thickness(11);
        draw(&mut strokes, Tool::Arrow, (0, 0), (60, 30));
        let first = strokes.annotations().to_vec();
        draw(&mut strokes, Tool::Line, (100, 0), (100, 30));
        let both = strokes.annotations().to_vec();
        assert!(strokes.undo());
        let undo = strokes.undo.clone();
        let redo = strokes.redo.clone();
        let style = strokes.style();
        for _ in 0..2 {
            assert!(!strokes.toggle_temporary_pen());
            assert!(!strokes.toggle_arrow_direction());
            assert_eq!(strokes.tool(), Tool::Line);
            assert_eq!(strokes.style(), style);
            assert_eq!(strokes.annotations(), first);
            assert_eq!(strokes.undo, undo);
            assert_eq!(strokes.redo, redo);
        }
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), both);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), first);
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        assert!(!strokes.undo());
    }

    #[test]
    fn variant_toggles_cancel_shape_previews_without_losing_history() {
        for toggle in [
            Strokes::toggle_temporary_pen,
            Strokes::toggle_arrow_direction,
        ] {
            for tool in [Tool::Line, Tool::Rectangle, Tool::Arrow, Tool::Ellipse] {
                for tail_first in [false, true] {
                    let mut strokes = Strokes::default();
                    if tail_first {
                        strokes.toggle_arrow_direction();
                    }
                    draw(&mut strokes, Tool::Pen, (0, 0), (30, 30));
                    draw(&mut strokes, Tool::Pen, (100, 0), (130, 30));
                    assert!(strokes.undo());
                    let scene = strokes.annotations().to_vec();
                    let undo = strokes.undo.clone();
                    let redo = strokes.redo.clone();
                    let style = strokes.style();
                    strokes.select(tool);
                    strokes.begin((50, 50));
                    strokes.append((100, 100));
                    let preview = strokes.annotations().to_vec();
                    assert!(!strokes.select(tool)); // Same-tool selection stays a no-op.
                    assert!(strokes.is_active());
                    assert_eq!(strokes.annotations(), preview);
                    assert!(toggle(&mut strokes));
                    assert!(!strokes.is_active());
                    assert!(!strokes.append((200, 200)));
                    assert_eq!(strokes.tool(), tool);
                    assert_eq!(strokes.style(), style);
                    assert_eq!(strokes.annotations(), scene);
                    assert_eq!(strokes.undo, undo);
                    assert_eq!(strokes.redo, redo);
                    assert!(strokes.redo());
                    assert_eq!(strokes.annotations().len(), 2);
                }
            }
        }
    }

    #[test]
    fn variant_toggles_settle_pen_and_eraser_as_one_history_action() {
        for toggle in [
            Strokes::toggle_temporary_pen,
            Strokes::toggle_arrow_direction,
        ] {
            for tool in [Tool::Pen, Tool::Eraser] {
                let mut strokes = Strokes::default();
                draw(&mut strokes, Tool::Pen, (0, 0), (30, 30));
                draw(&mut strokes, Tool::Pen, (100, 0), (130, 30));
                assert!(strokes.undo());
                let before = strokes.annotations().to_vec();
                let history_len = strokes.undo.len();
                strokes.select(tool);
                strokes.begin((0, 0));
                strokes.append((30, 30));
                let settled = strokes.annotations().to_vec();
                assert!(!toggle(&mut strokes)); // Settling changes history, not pixels.
                assert!(!strokes.is_active());
                assert!(!strokes.append((50, 50)));
                assert_eq!(strokes.annotations(), settled);
                assert_eq!(strokes.undo.len(), history_len + 1);
                assert!(strokes.redo.is_empty()); // The edit, not the flag, replaces redo.
                assert!(!strokes.redo());
                assert!(strokes.undo());
                assert_eq!(strokes.annotations(), before);
                assert!(strokes.redo());
                assert_eq!(strokes.annotations(), settled);
                assert!(!strokes.redo());
            }
        }
    }

    #[test]
    fn styles_are_opaque_odd_bounded_and_frozen() {
        let mut strokes = Strokes::default();
        for width in 0..=40 {
            strokes.set_thickness(width);
            assert_eq!(strokes.style().thickness, width.clamp(1, 31) | 1);
        }
        strokes.set_thickness(u32::MAX);
        assert_eq!(strokes.style().thickness, 31);
        assert!(strokes.set_color(0x12010203));
        assert_eq!(strokes.style().color, 0xff010203);
        assert!(!strokes.set_color(0x99010203));
        let frozen = strokes.style();
        strokes.begin((1, 2));
        strokes.set_thickness(0);
        strokes.set_color(0x00abcdef);
        strokes.append((3, 4));
        strokes.finish();
        assert_eq!(strokes.annotations()[0].style, frozen);
        strokes.begin((5, 6));
        assert_eq!(strokes.annotations()[1].style, strokes.style());
        assert_eq!(strokes.annotations()[1].style.thickness, 1);
    }

    #[test]
    fn pen_dots_duplicates_separation_and_interruption() {
        let mut strokes = Strokes::default();
        assert!(strokes.begin((-12, 4)));
        assert!(!strokes.append((-12, 4)));
        assert!(strokes.append((3, 4)));
        assert!(!strokes.append((3, 4)));
        assert!(strokes.append((-12, 4)));
        assert!(!strokes.cancel());
        assert!(!strokes.is_active());
        assert!(!strokes.append((5, 6)));
        let path = &strokes.annotations()[0].paths[0];
        assert_eq!(path.first(), Some(&(-12, 4)));
        assert_eq!(path.last(), Some(&(-12, 4)));
        assert!(path.iter().any(|point| point.0 > -12));
        assert!(path.iter().all(|point| point.1 == 4));
        assert!(path.windows(2).all(|pair| pair[0] != pair[1]));
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        assert!(strokes.redo());
        strokes.begin((5, 6));
        strokes.begin((7, 8));
        strokes.finish();
        strokes.finish();
        assert_eq!(strokes.annotations().len(), 3);
        assert_eq!(strokes.annotations()[1].paths, [vec![(5, 6)]]);
        assert_eq!(strokes.annotations()[2].paths, [vec![(7, 8)]]);
    }

    #[test]
    fn bezier_pen_live_tail_finish_and_frozen_prefix() {
        let mut strokes = Strokes::default();
        strokes.begin((0, 0));
        strokes.append((100, 100));
        assert_eq!(strokes.annotations()[0].paths[0], [(0, 0), (100, 100)]);
        strokes.set_color(0xffabcdef);
        strokes.set_thickness(31);
        strokes.append((200, 0));
        let path = &strokes.annotations()[0].paths[0];
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(200, 0)));
        assert!(!path.contains(&(100, 100)));
        assert!(path.iter().any(|&(x, y)| x > y && y > 0 && x < 150));
        let finalized = path[..strokes.pen_tail].to_vec();
        strokes.append((250, 100));
        assert!(strokes.annotations()[0].paths[0].starts_with(&finalized));
        assert_eq!(strokes.annotations()[0].paths[0].last(), Some(&(250, 100)));
        let preview = strokes.annotations().to_vec();
        strokes.finish();
        strokes.finish();
        assert_eq!(strokes.annotations(), preview);
        assert_eq!(strokes.annotations()[0].style, Style::default());
        assert_eq!(strokes.undo.len(), 1);
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), preview);
    }

    #[test]
    fn bezier_pen_independent_gestures_and_small_reversals() {
        let mut strokes = Strokes::default();
        for (start, turn) in [
            ((0, 0), (1, -1)),
            ((100, 100), (101, 99)),
            ((-1, -1), (0, 0)),
            ((1, 1), (0, 0)),
            ((i32::MIN, i32::MAX), (i32::MIN + 1, i32::MAX - 1)),
        ] {
            strokes.begin(start);
            assert!(!strokes.append(start));
            strokes.append(turn);
            strokes.append(start);
            let path = &strokes.annotations().last().unwrap().paths[0];
            assert_eq!(path.first(), Some(&start));
            assert_eq!(path.last(), Some(&start));
            assert!(path.len() > 1);
            assert!(path.windows(2).all(|pair| pair[0] != pair[1]));
            strokes.cancel();
        }
        assert_eq!(strokes.annotations().len(), 5);
        assert_eq!(strokes.undo.len(), 5);
    }

    #[test]
    fn bezier_eraser_hits_curve_not_raw_joint_and_restores_history() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Line, (-100, -100), (-50, -100));
        strokes.select(Tool::Pen);
        strokes.set_thickness(1);
        strokes.set_color(0xff123456);
        strokes.begin((0, 0));
        strokes.append((100, 100));
        strokes.append((200, 0));
        strokes.finish();
        let original = strokes.annotations().to_vec();
        let target = original[1].paths[0]
            .iter()
            .copied()
            .find(|&(x, y)| x > y + 10 && x < 125)
            .unwrap();
        assert!(point_distance_squared(target, (0, 0), (100, 100)) > 4.5_f64.powi(2));
        assert!(point_distance_squared(target, (100, 100), (200, 0)) > 4.5_f64.powi(2));
        strokes.select(Tool::Eraser);
        assert!(!strokes.begin((100, 100)));
        strokes.finish();
        let history_len = strokes.undo.len();
        assert!(strokes.begin(target));
        strokes.finish();
        assert_eq!(strokes.annotations(), &original[..1]);
        assert_eq!(strokes.undo.len(), history_len + 1);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), original);
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), &original[..1]);
    }

    #[test]
    fn shape_previews_replace_without_trails_and_cancel() {
        for tool in [Tool::Line, Tool::Rectangle, Tool::Arrow, Tool::Ellipse] {
            let mut strokes = Strokes::default();
            assert!(!strokes.select(tool));
            assert!(strokes.begin((1, 2)));
            assert!(!strokes.append((1, 2)));
            assert!(strokes.append((50, 60)));
            assert!(strokes.append((-30, -40)));
            assert!(!strokes.append((-30, -40)));
            assert_eq!(
                strokes.annotations()[0].paths,
                shape(tool, (1, 2), (-30, -40), 5)
            );
            let preview = strokes.annotations().to_vec();
            assert!(!strokes.select(tool));
            assert!(strokes.is_active());
            assert_eq!(strokes.annotations(), preview);
            assert!(strokes.begin((1, 2)));
            assert_eq!(strokes.annotations().len(), 1);
            assert!(!strokes.begin((1, 2))); // Identical replacement is no visual change.
            assert!(strokes.cancel());
            assert!(!strokes.cancel());
            assert!(strokes.annotations().is_empty());
            assert!(!strokes.undo());
            strokes.begin((1, 2));
            strokes.append((3, 4));
            assert!(strokes.select(Tool::Pen));
            assert!(!strokes.is_active());
            assert!(strokes.annotations().is_empty());
        }
    }

    #[test]
    fn line_and_rectangle_all_directions_and_degeneracies() {
        let anchor = (1, 2);
        for end in [(5, 6), (-3, 6), (-3, -4), (5, -4), (1, 6), (5, 2), anchor] {
            assert_eq!(shape(Tool::Line, anchor, end, 5), [vec![anchor, end]]);
            assert_eq!(
                shape(Tool::Rectangle, anchor, end, 5),
                [vec![
                    anchor,
                    (end.0, anchor.1),
                    end,
                    (anchor.0, end.1),
                    anchor,
                ]]
            );
        }
    }

    #[test]
    fn arrows_have_separate_branches_in_all_directions() {
        for end in [
            (60, 30),
            (60, -30),
            (-60, 30),
            (-60, -30),
            (60, 0),
            (0, 60),
            (-60, 0),
            (0, -60),
            (1, 1),
            (0, 0),
        ] {
            let paths = shape(Tool::Arrow, (0, 0), end, 5);
            assert_eq!(paths.len(), 3);
            assert!(paths.iter().all(|path| path.len() == 2));
            assert_eq!(paths[0], [(0, 0), end]);
            assert_eq!(paths[1][0], (0, 0));
            assert_eq!(paths[2][0], (0, 0));
            if end.0.abs() + end.1.abs() > 2 {
                assert_ne!(paths[1][1], paths[2][1]);
                for branch in &paths[1..] {
                    let toward_tail = branch[1].0 * end.0 + branch[1].1 * end.1;
                    assert!(toward_tail > 0);
                }
            }
        }
        assert_eq!(
            shape(Tool::Arrow, (0, 0), (60, 0), 5),
            [
                vec![(0, 0), (60, 0)],
                vec![(0, 0), (45, 23)],
                vec![(0, 0), (45, -23)]
            ]
        );
        for (start, end) in [
            ((i32::MIN, i32::MIN), (i32::MAX, i32::MAX)),
            ((i32::MAX, i32::MIN), (i32::MIN, i32::MAX)),
        ] {
            assert_eq!(shape(Tool::Arrow, start, end, 5).len(), 3);
        }
    }

    #[test]
    fn arrow_variants_reverse_only_new_arrow_endpoints() {
        let anchor = (10, 20);
        for tail_first in [false, true] {
            for end in [
                (70, 50),
                (70, -10),
                (-50, 50),
                (-50, -10),
                (70, 20),
                (10, 80),
                (-50, 20),
                (10, -40),
                (11, 21),
                anchor,
            ] {
                let mut strokes = Strokes::default();
                if tail_first {
                    assert!(!strokes.toggle_arrow_direction());
                }
                strokes.select(Tool::Arrow);
                assert!(strokes.begin(anchor));
                assert_eq!(
                    strokes.annotations()[0].paths,
                    vec![vec![anchor, anchor]; 3]
                );
                strokes.append(end);
                let (head, tail) = if tail_first {
                    (end, anchor)
                } else {
                    (anchor, end)
                };
                let paths = &strokes.annotations()[0].paths;
                assert_eq!(*paths, shape(Tool::Arrow, head, tail, 5));
                assert_eq!(paths[0], [head, tail]);
                assert!(paths.iter().all(|path| path.len() == 2 && path[0] == head));
                if (end.0 - anchor.0).abs() + (end.1 - anchor.1).abs() > 2 {
                    assert_ne!(paths[1][1], paths[2][1]);
                    for branch in &paths[1..] {
                        let toward_tail = (branch[1].0 - head.0) * (tail.0 - head.0)
                            + (branch[1].1 - head.1) * (tail.1 - head.1);
                        assert!(toward_tail > 0);
                    }
                }
                strokes.finish();
            }
        }
        for tool in [Tool::Pen, Tool::Line, Tool::Rectangle, Tool::Ellipse] {
            let mut strokes = Strokes::default();
            strokes.toggle_arrow_direction();
            draw(&mut strokes, tool, anchor, (70, 50));
            let expected = if tool == Tool::Pen {
                vec![vec![anchor, (70, 50)]]
            } else {
                shape(tool, anchor, (70, 50), 5)
            };
            assert_eq!(strokes.annotations()[0].paths, expected);
        }
    }

    #[test]
    fn arrow_direction_changes_leave_completed_geometry_and_styles_frozen() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Arrow, (200, 200), (500, 200));
        let head_first = strokes.annotations().to_vec();
        assert!(!strokes.toggle_arrow_direction());
        assert_eq!(strokes.annotations(), head_first);
        strokes.begin((200, 200));
        strokes.set_color(0xff123456);
        strokes.set_thickness(31);
        assert!(strokes.append((500, 200)));
        assert_eq!(
            strokes.annotations()[1].paths,
            [
                vec![(500, 200), (200, 200)],
                vec![(500, 200), (422, 161)],
                vec![(500, 200), (422, 239)],
            ]
        );
        assert_eq!(strokes.annotations()[1].style, head_first[0].style);
        assert!(strokes.append((200, 500)));
        assert_eq!(
            strokes.annotations()[1].paths,
            shape(Tool::Arrow, (200, 500), (200, 200), 5)
        );
        strokes.finish();
        let both = strokes.annotations().to_vec();
        assert!(!strokes.toggle_arrow_direction());
        assert!(!strokes.toggle_temporary_pen());
        assert_eq!(strokes.annotations(), both);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), head_first);
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), both);
        assert!(!strokes.arrow_tail_first());
        assert!(strokes.temporary_pen());
    }

    #[test]
    fn arrow_head_scale_defaults_and_half_double_geometry() {
        let anchor = (23, -41);
        assert_eq!(Strokes::default().arrow_head_scale(), 1.0);
        for scale in [0.5, 1.0, 2.0] {
            for width in (1..=31).step_by(2) {
                for tail_first in [false, true] {
                    for (dx, dy) in [
                        (1, 0),
                        (1, 1),
                        (0, 1),
                        (-1, 1),
                        (-1, 0),
                        (-1, -1),
                        (0, -1),
                        (1, -1),
                    ] {
                        let mut strokes = Strokes::default();
                        assert_eq!(strokes.set_arrow_head_scale(scale), scale != 1.0);
                        strokes.set_thickness(width);
                        if tail_first {
                            strokes.toggle_arrow_direction();
                        }
                        let end = (anchor.0 + dx * 1000, anchor.1 + dy * 1000);
                        draw(&mut strokes, Tool::Arrow, anchor, end);
                        let (head, tail) = if tail_first {
                            (end, anchor)
                        } else {
                            (anchor, end)
                        };
                        let paths = &strokes.annotations()[0].paths;
                        if scale == 1.0 {
                            assert_eq!(*paths, shape(Tool::Arrow, head, tail, width));
                        }
                        assert_eq!(paths[0], [head, tail]);
                        let (vx, vy) = ((tail.0 - head.0) as f64, (tail.1 - head.1) as f64);
                        let length = vx.hypot(vy);
                        let reach = (48.0 + 6.0 * width as f64) * scale;
                        for (wing, side) in paths[1..].iter().zip([1.0, -1.0]) {
                            assert_eq!(wing[0], head);
                            let (x, y) = ((wing[1].0 - head.0) as f64, (wing[1].1 - head.1) as f64);
                            assert!(((x * vx + y * vy) / length - reach).abs() <= 0.71);
                            assert!(
                                ((y * vx - x * vy) / length - side * reach / 2.0).abs() <= 0.71
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn arrow_head_scale_caps_after_scaling_and_handles_zero_length() {
        for width in [1, 5, 31] {
            for scale in [0.1, 0.5, 1.0, 2.0, 4.0] {
                let reach = ((48.0 + 6.0 * width as f64) * scale).min(60.0);
                assert_eq!(
                    shape_with_head_scale(Tool::Arrow, (0, 0), (80, 0), width, scale),
                    [
                        vec![(0, 0), (80, 0)],
                        vec![(0, 0), (reach.round() as i32, (reach / 2.0).round() as i32)],
                        vec![
                            (0, 0),
                            (reach.round() as i32, (-reach / 2.0).round() as i32)
                        ],
                    ]
                );
                for end in [(1, 0), (0, 1), (-1, -1)] {
                    let paths = shape_with_head_scale(Tool::Arrow, (0, 0), end, width, scale);
                    assert_eq!(paths[0], [(0, 0), end]);
                    assert!(
                        paths
                            .iter()
                            .flatten()
                            .all(|&(x, y)| x.abs() <= 2 && y.abs() <= 2)
                    );
                }
                assert_eq!(
                    shape_with_head_scale(Tool::Arrow, (2, -3), (2, -3), width, scale),
                    vec![vec![(2, -3), (2, -3)]; 3]
                );
            }
        }
    }

    #[test]
    fn arrow_head_scale_invalid_and_unchanged_values_preserve_gestures() {
        for tool in [
            Tool::Pen,
            Tool::Line,
            Tool::Rectangle,
            Tool::Arrow,
            Tool::Ellipse,
            Tool::Eraser,
        ] {
            let mut strokes = Strokes::default();
            draw(&mut strokes, Tool::Pen, (0, 0), (30, 30));
            draw(&mut strokes, Tool::Line, (100, 0), (130, 30));
            assert!(strokes.undo());
            strokes.select(tool);
            strokes.begin((0, 0));
            strokes.append((30, 30));
            let preview = strokes.annotations().to_vec();
            let before = strokes.before.clone();
            let undo = strokes.undo.clone();
            let redo = strokes.redo.clone();
            for scale in [
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                -1.0,
                -0.0,
                0.0,
                0.099,
                4.001,
                1.0,
            ] {
                assert!(!strokes.set_arrow_head_scale(scale));
                assert_eq!(strokes.arrow_head_scale(), 1.0);
                assert!(strokes.is_active());
                assert_eq!(strokes.annotations(), preview);
                assert_eq!(strokes.before, before);
                assert_eq!(strokes.undo, undo);
                assert_eq!(strokes.redo, redo);
            }
            assert!(strokes.set_arrow_head_scale(0.1));
            assert!(!strokes.is_active());
            assert_eq!(strokes.arrow_head_scale(), 0.1);
            assert!(!strokes.set_arrow_head_scale(0.1));
            assert!(strokes.set_arrow_head_scale(4.0));
            assert_eq!(strokes.arrow_head_scale(), 4.0);
            assert!(!strokes.set_arrow_head_scale(4.0));
        }
    }

    #[test]
    fn arrow_head_scale_preserves_completed_geometry_and_undo_redo() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Arrow, (0, 0), (1000, 0));
        let first = strokes.annotations().to_vec();
        strokes.set_thickness(31);
        strokes.toggle_arrow_direction();
        assert!(strokes.set_arrow_head_scale(2.0));
        draw(&mut strokes, Tool::Arrow, (0, 0), (1000, 0));
        let both = strokes.annotations().to_vec();
        assert_eq!(both[0], first[0]);
        assert_eq!(both[1].paths[1], [(1000, 0), (532, -234)]);
        assert!(strokes.undo());
        let undo = strokes.undo.clone();
        let redo = strokes.redo.clone();
        assert!(strokes.set_arrow_head_scale(0.5));
        assert_eq!(strokes.annotations(), first);
        assert_eq!(strokes.undo, undo);
        assert_eq!(strokes.redo, redo);
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), both);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), first);
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        assert!(strokes.redo());
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), both);
        assert_eq!(strokes.arrow_head_scale(), 0.5);
    }

    #[test]
    fn arrow_head_scale_settles_active_previews_and_pen_eraser_edits() {
        for tool in [
            Tool::Pen,
            Tool::Line,
            Tool::Rectangle,
            Tool::Arrow,
            Tool::Ellipse,
            Tool::Eraser,
        ] {
            for tail_first in [false, true] {
                let mut strokes = Strokes::default();
                draw(&mut strokes, Tool::Pen, (0, 0), (30, 30));
                draw(&mut strokes, Tool::Arrow, (100, 0), (1000, 0));
                assert!(strokes.undo());
                let scene = strokes.annotations().to_vec();
                let undo = strokes.undo.clone();
                let redo = strokes.redo.clone();
                if tail_first {
                    strokes.toggle_arrow_direction();
                }
                strokes.select(tool);
                let selection = strokes.selection();
                let previous_tool = strokes.previous_tool;
                let style = strokes.style();
                strokes.begin((0, 0));
                strokes.append((30, 30));
                let preview = strokes.annotations().to_vec();
                assert!(strokes.set_arrow_head_scale(2.0));
                assert!(!strokes.is_active());
                assert!(!strokes.append((200, 200)));
                assert_eq!(strokes.selection(), selection);
                assert_eq!(strokes.previous_tool, previous_tool);
                assert_eq!(strokes.style(), style);
                if matches!(tool, Tool::Pen | Tool::Eraser) {
                    assert_eq!(strokes.annotations(), preview);
                    assert_eq!(strokes.undo.len(), undo.len() + 1);
                    assert!(strokes.redo.is_empty());
                    assert!(strokes.undo());
                    assert_eq!(strokes.annotations(), scene);
                    assert!(strokes.redo());
                    assert_eq!(strokes.annotations(), preview);
                } else {
                    assert_eq!(strokes.annotations(), scene);
                    assert_eq!(strokes.undo, undo);
                    assert_eq!(strokes.redo, redo);
                    assert!(strokes.redo());
                    assert_eq!(strokes.annotations().len(), 2);
                }
            }
        }
    }

    #[test]
    fn arrow_head_scales_with_width_and_short_gestures_stay_bounded() {
        let mut previous = 0;
        for width in (1..=31).step_by(2) {
            let paths = shape(Tool::Arrow, (0, 0), (500, 0), width);
            let head = paths[1][1];
            assert_eq!(head.0, 48 + 6 * width as i32);
            assert!(head.0 > previous);
            assert!(head.1 * 2 > width as i32 * 3);
            assert_eq!(paths[2][1], (head.0, -head.1));
            previous = head.0;
            for end in [(1, 0), (0, 1), (-1, -1), (0, 0)] {
                let short = shape(Tool::Arrow, (0, 0), end, width);
                assert_eq!(short[1][0], (0, 0));
                assert!(
                    short
                        .iter()
                        .flatten()
                        .all(|&(x, y)| x.abs() <= 2 && y.abs() <= 2)
                );
            }
        }
    }

    #[test]
    fn arrow_preview_locks_the_head_and_uses_its_frozen_width() {
        let mut strokes = Strokes::default();
        strokes.select(Tool::Arrow);
        strokes.begin((200, 200));
        strokes.set_thickness(31);
        assert!(strokes.append((500, 200)));
        assert_eq!(strokes.annotations()[0].paths[1], [(200, 200), (278, 239)]);
        assert_eq!(strokes.annotations()[0].style.thickness, 5);
        assert!(strokes.append((200, 500)));
        assert_eq!(strokes.annotations()[0].paths[0], [(200, 200), (200, 500)]);
        assert_eq!(strokes.annotations()[0].paths[1][0], (200, 200));
        strokes.finish();
        let completed = strokes.annotations().to_vec();
        strokes.begin((200, 200));
        strokes.append((500, 200));
        assert_eq!(strokes.annotations()[1].paths[1], [(200, 200), (425, 313)]);
        assert!(strokes.cancel());
        assert_eq!(strokes.annotations(), completed);
        assert!(strokes.undo());
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), completed);
    }

    #[test]
    fn ellipses_are_closed_bounded_and_direction_independent() {
        let canonical = shape(Tool::Ellipse, (-10, -20), (30, 40), 5);
        for (start, end) in [
            ((-10, -20), (30, 40)),
            ((30, 40), (-10, -20)),
            ((-10, 40), (30, -20)),
            ((30, -20), (-10, 40)),
        ] {
            assert_eq!(shape(Tool::Ellipse, start, end, 5), canonical);
        }
        for (start, end) in [
            ((0, 0), (1, 1)),
            ((0, 0), (10000, 10000)),
            ((i32::MIN, i32::MIN), (i32::MAX, i32::MAX)),
        ] {
            let paths = shape(Tool::Ellipse, start, end, 5);
            let path = &paths[0];
            assert_eq!(path.first(), path.last());
            assert!((13..=2049).contains(&path.len()));
            assert!(
                path.iter()
                    .all(|&(x, y)| x >= start.0 && x <= end.0 && y >= start.1 && y <= end.1)
            );
            assert_eq!(path.iter().map(|p| p.0).min(), Some(start.0));
            assert_eq!(path.iter().map(|p| p.0).max(), Some(end.0));
            assert_eq!(path.iter().map(|p| p.1).min(), Some(start.1));
            assert_eq!(path.iter().map(|p| p.1).max(), Some(end.1));
        }
        for end in [(0, 0), (0, 100), (100, 0), (0, -100), (-100, 0)] {
            assert_eq!(
                shape(Tool::Ellipse, (0, 0), end, 5),
                [vec![(0, 0), end, (0, 0)]]
            );
        }
    }

    #[test]
    fn poc7_arrow_heads_use_larger_normalized_widths_and_keep_short_cap() {
        assert_eq!(Style::default().thickness, 5);
        for (width, reach) in [
            (0, 54),
            (4, 78),
            (5, 78),
            (30, 234),
            (31, 234),
            (u32::MAX, 234),
        ] {
            assert_eq!(
                shape(Tool::Arrow, (0, 0), (1000, 0), width),
                [
                    vec![(0, 0), (1000, 0)],
                    vec![(0, 0), (reach, reach / 2)],
                    vec![(0, 0), (reach, -reach / 2)],
                ]
            );
            assert_eq!(
                shape(Tool::Arrow, (0, 0), (80, 0), width),
                [
                    vec![(0, 0), (80, 0)],
                    vec![(0, 0), (reach.min(60), reach.min(60) / 2)],
                    vec![(0, 0), (reach.min(60), -reach.min(60) / 2)],
                ]
            );
        }
    }

    #[test]
    fn poc7_arrow_wings_have_larger_dimensions_in_all_directions_and_tail_first() {
        let anchor = (23, -41);
        for (width, reach) in [(5, 78.0), (31, 234.0)] {
            for tail_first in [false, true] {
                for (dx, dy) in [
                    (1, 0),
                    (1, 1),
                    (0, 1),
                    (-1, 1),
                    (-1, 0),
                    (-1, -1),
                    (0, -1),
                    (1, -1),
                ] {
                    let mut strokes = Strokes::default();
                    strokes.set_thickness(width);
                    if tail_first {
                        strokes.toggle_arrow_direction();
                    }
                    let end = (anchor.0 + dx * 700, anchor.1 + dy * 700);
                    draw(&mut strokes, Tool::Arrow, anchor, end);
                    let (head, tail) = if tail_first {
                        (end, anchor)
                    } else {
                        (anchor, end)
                    };
                    let paths = &strokes.annotations()[0].paths;
                    assert_eq!(paths.len(), 3);
                    assert_eq!(paths[0], [head, tail]);
                    let (vx, vy) = ((tail.0 - head.0) as f64, (tail.1 - head.1) as f64);
                    let length = vx.hypot(vy);
                    for (wing, side) in paths[1..].iter().zip([1.0, -1.0]) {
                        assert_eq!(wing.len(), 2);
                        assert_eq!(wing[0], head);
                        let (x, y) = ((wing[1].0 - head.0) as f64, (wing[1].1 - head.1) as f64);
                        assert!(((x * vx + y * vy) / length - reach).abs() <= 0.71);
                        assert!(((y * vx - x * vy) / length - side * reach / 2.0).abs() <= 0.71);
                    }
                    assert_ne!(paths[1][1], paths[2][1]);
                }
            }
        }
    }

    #[test]
    fn poc7_large_ellipses_bound_error_rounding_and_preserve_closure_extrema() {
        for (rx, ry) in [(10000, 3000), (3000, 10000), (200000, 50000)] {
            let canonical = shape(Tool::Ellipse, (-rx, -ry), (rx, ry), 5);
            for (start, end) in [
                ((-rx, -ry), (rx, ry)),
                ((rx, ry), (-rx, -ry)),
                ((-rx, ry), (rx, -ry)),
                ((rx, -ry), (-rx, ry)),
            ] {
                assert_eq!(shape(Tool::Ellipse, start, end, 5), canonical);
            }
            let path = &canonical[0];
            let steps = path.len() - 1;
            assert!((132..=2048).contains(&steps));
            assert_eq!(steps % 4, 0);
            assert_eq!(path.first(), path.last());
            assert_eq!(path[0], (rx, 0));
            assert_eq!(path[steps / 4], (0, ry));
            assert_eq!(path[steps / 2], (-rx, 0));
            assert_eq!(path[3 * steps / 4], (0, -ry));
            assert!(
                f64::from(rx.max(ry)) * (1.0 - (std::f64::consts::PI / steps as f64).cos()) <= 0.25
            );
            for (step, segment) in path.windows(2).enumerate() {
                let angle = std::f64::consts::TAU * step as f64 / steps as f64;
                assert!((f64::from(segment[0].0) - f64::from(rx) * angle.cos()).abs() <= 0.500001);
                assert!((f64::from(segment[0].1) - f64::from(ry) * angle.sin()).abs() <= 0.500001);
                let midpoint_angle = angle + std::f64::consts::PI / steps as f64;
                let chord_x = (f64::from(segment[0].0) + f64::from(segment[1].0)) / 2.0;
                let chord_y = (f64::from(segment[0].1) + f64::from(segment[1].1)) / 2.0;
                let error = (chord_x - f64::from(rx) * midpoint_angle.cos())
                    .hypot(chord_y - f64::from(ry) * midpoint_angle.sin());
                // Integer rounding adds at most sqrt(0.5)px to the tessellation budget.
                assert!(error <= 0.25 + 0.5_f64.sqrt() + 0.000001);
            }
        }
        assert_eq!(shape(Tool::Ellipse, (-1, -1), (1, 1), 5)[0].len(), 13);
        let extreme = shape(Tool::Ellipse, (i32::MIN, i32::MIN), (i32::MAX, i32::MAX), 5);
        assert_eq!(extreme[0].len(), 2049);
        assert_eq!(extreme[0].first(), extreme[0].last());
        assert_eq!(extreme[0][0].0, i32::MAX);
        assert_eq!(extreme[0][512].1, i32::MAX);
        assert_eq!(extreme[0][1024].0, i32::MIN);
        assert_eq!(extreme[0][1536].1, i32::MIN);
    }

    #[test]
    fn poc7_eraser_hits_new_arrow_wings_and_restores_frozen_geometry_and_history() {
        for width in [5, 31] {
            for tail_first in [false, true] {
                let mut strokes = Strokes::default();
                draw(&mut strokes, Tool::Line, (-1000, -1000), (-900, -1000));
                strokes.set_color(0xff123456);
                strokes.set_thickness(width);
                if tail_first {
                    strokes.toggle_arrow_direction();
                }
                draw(&mut strokes, Tool::Arrow, (0, 0), (1000, 0));
                let original = strokes.annotations().to_vec();
                strokes.set_color(0xffabcdef);
                strokes.set_thickness(1);
                strokes.select(Tool::Eraser);
                for wing in &original[1].paths[1..] {
                    let history_len = strokes.undo.len();
                    assert!(strokes.begin(wing[1]));
                    strokes.finish();
                    assert_eq!(strokes.annotations(), &original[..1]);
                    assert_eq!(strokes.undo.len(), history_len + 1);
                    assert!(strokes.undo());
                    assert_eq!(strokes.annotations(), original);
                    assert!(strokes.redo());
                    assert_eq!(strokes.annotations(), &original[..1]);
                    assert!(strokes.undo());
                    assert_eq!(strokes.annotations(), original);
                }
            }
        }
    }

    #[test]
    fn poc7_eraser_hits_large_ellipse_segments_and_closure_and_restores_history() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Line, (-1000, -1000), (-900, -1000));
        strokes.set_color(0xff123456);
        strokes.set_thickness(1);
        draw(&mut strokes, Tool::Ellipse, (0, 0), (100000, 60000));
        let original = strokes.annotations().to_vec();
        let path = &original[1].paths[0];
        let steps = path.len() - 1;
        assert!(steps > 128);
        strokes.select(Tool::Eraser);
        for index in [steps / 256, steps / 8, steps - 1] {
            let target = (
                (path[index].0 + path[index + 1].0) / 2,
                (path[index].1 + path[index + 1].1) / 2,
            );
            let history_len = strokes.undo.len();
            assert!(strokes.begin(target));
            strokes.finish();
            assert_eq!(strokes.annotations(), &original[..1]);
            assert_eq!(strokes.undo.len(), history_len + 1);
            assert!(strokes.undo());
            assert_eq!(strokes.annotations(), original);
            assert!(strokes.redo());
            assert_eq!(strokes.annotations(), &original[..1]);
            assert!(strokes.undo());
            assert_eq!(strokes.annotations(), original);
        }
    }

    #[test]
    fn history_restores_styles_order_clear_and_branches() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Pen, (1, 2), (3, 4));
        let first = strokes.annotations().to_vec();
        strokes.set_color(0xff123456);
        strokes.set_thickness(11);
        draw(&mut strokes, Tool::Rectangle, (5, 6), (7, 8));
        let both = strokes.annotations().to_vec();
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), first);
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), both);
        assert!(!strokes.redo());
        assert!(strokes.clear());
        assert!(!strokes.clear());
        assert_eq!(strokes.tool(), Tool::Rectangle);
        assert_eq!(strokes.style(), both[1].style);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), both);
        assert!(strokes.redo());
        assert!(strokes.annotations().is_empty());
        assert!(strokes.undo());
        assert!(strokes.undo());
        draw(&mut strokes, Tool::Line, (9, 10), (11, 12));
        assert!(!strokes.redo());
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), first);
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        assert!(!strokes.undo());
    }

    #[test]
    fn history_settles_pen_and_cancels_shapes_without_losing_redo() {
        let mut strokes = Strokes::default();
        strokes.begin((1, 2));
        strokes.append((3, 4));
        let pen = strokes.annotations().to_vec();
        assert!(strokes.undo());
        assert!(!strokes.is_active());
        strokes.select(Tool::Ellipse);
        strokes.begin((10, 10));
        strokes.append((100, 100));
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), pen);
        strokes.begin((10, 10));
        assert!(strokes.undo());
        assert!(strokes.annotations().is_empty());
        strokes.begin((10, 10));
        assert!(strokes.undo()); // Only the canceled preview changed the scene.
        assert!(!strokes.undo());
        assert!(strokes.redo());
        strokes.select(Tool::Pen);
        strokes.begin((5, 6));
        assert!(!strokes.redo()); // Settling this new pen invalidates redo.
        assert!(!strokes.is_active());
        assert_eq!(strokes.annotations().len(), 2);
    }

    #[test]
    fn clear_settles_gestures_and_is_itself_undoable() {
        for tool in [
            Tool::Pen,
            Tool::Line,
            Tool::Rectangle,
            Tool::Arrow,
            Tool::Ellipse,
        ] {
            let mut strokes = Strokes::default();
            draw(&mut strokes, Tool::Pen, (1, 2), (3, 4));
            let pen = strokes.annotations().to_vec();
            strokes.select(tool);
            strokes.begin((5, 6));
            strokes.append((7, 8));
            let active = strokes.annotations().to_vec();
            assert!(strokes.clear());
            assert!(!strokes.is_active());
            assert!(!strokes.append((9, 10)));
            assert!(strokes.annotations().is_empty());
            assert!(strokes.undo());
            assert_eq!(
                strokes.annotations(),
                if tool == Tool::Pen { &active } else { &pen }
            );
        }
        let mut strokes = Strokes::default();
        strokes.select(Tool::Rectangle);
        strokes.begin((1, 2));
        assert!(strokes.clear());
        assert!(!strokes.undo()); // No canceled preview in history.
    }

    #[test]
    fn eraser_removes_topmost_annotation_and_restores_style_order() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Line, (0, 0), (100, 0));
        strokes.set_color(0xff123456);
        strokes.set_thickness(11);
        draw(&mut strokes, Tool::Line, (0, 0), (100, 0));
        let original = strokes.annotations().to_vec();
        strokes.select(Tool::Eraser);
        assert!(strokes.begin((50, 0)));
        assert_eq!(strokes.annotations(), &original[..1]);
        assert!(!strokes.append((50, 0)));
        assert!(!strokes.cancel()); // Eraser interruption commits, does not restore.
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), original);
        assert!(strokes.redo());
        assert_eq!(strokes.annotations(), &original[..1]);
        assert!(strokes.undo());
        strokes.begin((50, 5)); // Width is part of visible geometry.
        strokes.finish();
        assert_eq!(strokes.annotations(), &original[..1]);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), original);
    }

    #[test]
    fn eraser_uses_outlines_not_interiors_or_arrow_branch_connections() {
        for tool in [Tool::Rectangle, Tool::Ellipse] {
            let mut strokes = Strokes::default();
            draw(&mut strokes, tool, (0, 0), (100, 100));
            let original = strokes.annotations().to_vec();
            strokes.select(Tool::Eraser);
            assert!(!strokes.begin((50, 50)));
            assert!(!strokes.append((60, 50)));
            strokes.finish();
            assert_eq!(strokes.annotations(), original);
            assert!(strokes.begin((100, 50)));
            assert!(strokes.annotations().is_empty());
            strokes.finish();
            assert!(strokes.undo());
            assert_eq!(strokes.annotations(), original);
            assert!(strokes.undo()); // Empty eraser gesture did not enter history.
            assert!(strokes.annotations().is_empty());
        }
        let mut strokes = Strokes::default();
        strokes.set_thickness(1);
        draw(&mut strokes, Tool::Arrow, (0, 0), (60, 0));
        strokes.select(Tool::Eraser);
        assert!(!strokes.begin((45, 8))); // Would hit a falsely joined branch path.
        assert!(strokes.append((45, 23)));
        assert!(strokes.annotations().is_empty());
    }

    #[test]
    fn eraser_skips_top_shape_interiors_and_preserves_redo_on_misses() {
        for tool in [Tool::Rectangle, Tool::Ellipse] {
            let mut strokes = Strokes::default();
            draw(&mut strokes, Tool::Line, (30, 50), (70, 50));
            strokes.set_color(0xff123456);
            draw(&mut strokes, tool, (0, 0), (100, 100));
            let original = strokes.annotations().to_vec();
            strokes.select(Tool::Eraser);
            assert!(strokes.begin((50, 50)));
            strokes.finish();
            assert_eq!(strokes.annotations(), &original[1..]);
            assert!(strokes.undo());
            assert_eq!(strokes.annotations(), original);
            assert!(!strokes.begin((200, 200)));
            assert!(!strokes.append((300, 300)));
            assert!(!strokes.cancel());
            assert!(strokes.redo());
            assert_eq!(strokes.annotations(), &original[1..]);
            assert!(strokes.undo());
            assert!(!strokes.begin((-50, 50)));
            assert!(strokes.append((150, 50)));
            strokes.finish();
            assert!(strokes.annotations().is_empty()); // Moving sweeps can remove overlapping layers.
            assert!(strokes.undo());
            assert_eq!(strokes.annotations(), original);
        }
    }

    #[test]
    fn sparse_erase_sweeps_and_entire_drag_is_one_history_action() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Pen, (0, 0), (0, 0));
        strokes.set_color(0xff123456);
        draw(&mut strokes, Tool::Line, (100, -20), (100, 20));
        let original = strokes.annotations().to_vec();
        strokes.select(Tool::Eraser);
        assert!(strokes.begin((0, 0)));
        assert!(strokes.append((200, 0))); // No event at the crossed line itself.
        assert!(!strokes.append((300, 0)));
        strokes.finish();
        assert!(strokes.annotations().is_empty());
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), original);
        assert!(strokes.redo());
        assert!(strokes.annotations().is_empty());
        assert!(!strokes.redo());
        assert!(strokes.undo());
        strokes.begin((0, 0));
        strokes.append((200, 0));
        assert!(strokes.undo()); // Active eraser settles as a single action before undo.
        assert_eq!(strokes.annotations(), original);
        assert!(!strokes.is_active());
    }

    #[test]
    fn sparse_sweep_erases_three_disjoint_lines_and_restores_one_transaction() {
        let mut strokes = Strokes::default();
        for (x, color, thickness) in [
            (-100, 0xff123456, 1),
            (0, 0xffabcdef, 5),
            (100, 0xff987654, 11),
        ] {
            strokes.set_color(color);
            strokes.set_thickness(thickness);
            draw(&mut strokes, Tool::Line, (x, -20), (x, 20));
        }
        let original = strokes.annotations().to_vec();
        let history_len = strokes.undo.len();
        strokes.select(Tool::Eraser);
        assert!(!strokes.begin((i32::MIN, 200)));
        assert!(!strokes.append((i32::MAX, 200)));
        strokes.finish();
        assert_eq!(strokes.annotations(), original);
        assert_eq!(strokes.undo.len(), history_len);
        assert!(!strokes.begin((i32::MIN, 0)));
        assert!(strokes.append((i32::MAX, 0)));
        assert!(strokes.annotations().is_empty());
        assert!(!strokes.append((i32::MAX, 0))); // Same-coordinate MouseUp.
        strokes.finish();
        assert_eq!(strokes.undo.len(), history_len + 1);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), original); // Includes every style and stacking order.
        assert!(strokes.redo());
        assert!(strokes.annotations().is_empty());
    }

    #[test]
    fn square_dot_corner_click_removes_only_topmost_without_duplicate_on_up() {
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Pen, (10, 10), (10, 10));
        strokes.set_color(0xff123456);
        draw(&mut strokes, Tool::Pen, (10, 10), (10, 10));
        let original = strokes.annotations().to_vec();
        strokes.select(Tool::Eraser);
        assert!(strokes.begin((12, 12))); // Painted corner of a square 5px dot.
        assert_eq!(strokes.annotations(), &original[..1]);
        assert!(!strokes.append((12, 12))); // Same-coordinate MouseUp must not peel another layer.
        strokes.finish();
        assert_eq!(strokes.annotations(), &original[..1]);
        assert!(strokes.undo());
        assert_eq!(strokes.annotations(), original);
        assert!(!strokes.begin((17, 10))); // Beyond half-width plus 4px slop.
        assert!(!strokes.append((18, 10)));
        strokes.finish();
        assert!(strokes.redo()); // Misses did not invalidate redo.
        assert_eq!(strokes.annotations(), &original[..1]);
    }

    #[test]
    fn eraser_cancellation_selection_and_clear_commit_edits() {
        for settle in 0..3 {
            let mut strokes = Strokes::default();
            draw(&mut strokes, Tool::Pen, (0, 0), (0, 0));
            draw(&mut strokes, Tool::Pen, (100, 0), (100, 0));
            let original = strokes.annotations().to_vec();
            strokes.select(Tool::Eraser);
            strokes.begin((0, 0));
            match settle {
                0 => assert!(!strokes.cancel()),
                1 => assert!(!strokes.select(Tool::Line)),
                _ => {
                    assert!(strokes.clear());
                    assert!(strokes.undo());
                    assert_eq!(strokes.annotations(), &original[1..]);
                }
            }
            assert!(!strokes.is_active());
            assert!(strokes.undo());
            assert_eq!(strokes.annotations(), original);
            assert!(strokes.redo());
            assert_eq!(strokes.annotations(), &original[1..]);
        }
    }

    #[test]
    fn segment_distances_cover_crossings_parallel_degenerate_and_extremes() {
        assert_eq!(
            segment_distance_squared((0, 0), (10, 0), (5, -10), (5, 10)),
            0.0
        );
        assert_eq!(
            segment_distance_squared((0, 0), (10, 0), (5, 0), (15, 0)),
            0.0
        );
        assert_eq!(
            segment_distance_squared((0, 0), (10, 0), (11, 0), (20, 0)),
            1.0
        );
        assert_eq!(
            segment_distance_squared((0, 0), (10, 0), (0, 3), (10, 3)),
            9.0
        );
        assert_eq!(
            segment_distance_squared((0, 0), (0, 0), (3, 4), (3, 4)),
            25.0
        );
        assert_eq!(
            segment_distance_squared((i32::MIN, 0), (i32::MAX, 0), (0, i32::MIN), (0, i32::MAX)),
            0.0
        );
        let mut strokes = Strokes::default();
        draw(&mut strokes, Tool::Line, (i32::MIN, 0), (i32::MAX, 0));
        strokes.select(Tool::Eraser);
        assert!(!strokes.begin((0, i32::MIN)));
        assert!(strokes.append((0, i32::MAX)));
    }
}
