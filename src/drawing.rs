use crate::strokes::{Annotation, Point};

pub fn render(pixels: &mut [u32], width: usize, drawing: bool, annotations: &[Annotation]) {
    assert!(width > 0 && pixels.len().is_multiple_of(width));
    // ponytail: full-screen fill/upload and O(total stroke points) replay per change;
    // measure at the tested resolution before switching to a GPU renderer.
    pixels.fill(if drawing { 0x01_00_00_00 } else { 0 });
    if pixels.is_empty() {
        return;
    }
    for annotation in annotations {
        // Public styles may bypass the model's validation. Round even widths up.
        let radius = i64::from((annotation.style.thickness.clamp(1, 31) | 1) / 2);
        let color = annotation.style.color | 0xff_00_00_00; // Opaque premultiplied BGRA DIB.
        for path in &annotation.paths {
            if let Some(&point) = path.first() {
                line(pixels, width, point, point, radius, color);
            }
            for segment in path.windows(2) {
                line(pixels, width, segment[0], segment[1], radius, color);
            }
        }
    }
}

fn line(pixels: &mut [u32], width: usize, start: Point, end: Point, radius: i64, color: u32) {
    let (x, y) = (i64::from(start.0), i64::from(start.1));
    let (dx, dy) = ((i64::from(end.0) - x).abs(), (i64::from(end.1) - y).abs());
    let (sx, sy) = (
        if x < i64::from(end.0) { 1 } else { -1 },
        if y < i64::from(end.1) { 1 } else { -1 },
    );
    let height = (pixels.len() / width) as i64;
    let steps = dx.max(dy);
    let (origin, direction, limit) = if dx >= dy {
        (x, sx, width as i64)
    } else {
        (y, sy, height)
    };
    // Jump to visible major-axis steps without changing Bresenham's rounding.
    // i128 products cover the full i32 endpoint range without overflow.
    let (first, last) = if direction > 0 {
        (
            (-radius - origin).max(0),
            (limit - 1 + radius - origin).min(steps),
        )
    } else {
        (
            (origin - (limit - 1 + radius)).max(0),
            (origin + radius).min(steps),
        )
    };
    for step in first..=last {
        let offset = |delta| {
            ((i128::from(step) * i128::from(delta) + i128::from(steps / 2))
                / i128::from(steps.max(1))) as i64
        };
        let (x, y) = (x + sx * offset(dx), y + sy * offset(dy));
        for py in (y - radius).max(0)..=(y + radius).min(height - 1) {
            for px in (x - radius).max(0)..=(x + radius).min(width as i64 - 1) {
                pixels[py as usize * width + px as usize] = color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strokes::Style;

    const INK: u32 = 0xff_f9_73_16;

    fn annotation(paths: Vec<Vec<Point>>) -> Annotation {
        Annotation {
            paths,
            style: Style::default(),
        }
    }

    #[test]
    fn empty_startup_has_only_the_mode_background() {
        for drawing in [false, true] {
            let mut pixels = vec![INK; 80 * 60];
            render(&mut pixels, 80, drawing, &[]);
            let background = if drawing { 0x01000000 } else { 0 };
            assert!(pixels.iter().all(|&pixel| pixel == background));
        }
    }

    #[test]
    fn single_point_is_an_opaque_five_pixel_dot() {
        let mut pixels = vec![0; 11 * 11];
        render(&mut pixels, 11, false, &[annotation(vec![vec![(5, 5)]])]);
        for y in 0..11 {
            for x in 0..11 {
                let expected = if (3..=7).contains(&x) && (3..=7).contains(&y) {
                    INK
                } else {
                    0
                };
                assert_eq!(pixels[y * 11 + x], expected);
            }
        }
        assert_eq!(INK.to_le_bytes(), [0x16, 0x73, 0xf9, 0xff]);
    }

    #[test]
    fn sparse_segments_interpolate_in_each_direction() {
        for end in [
            (32, 26),
            (32, 14),
            (8, 26),
            (8, 14),
            (26, 32),
            (14, 32),
            (26, 8),
            (14, 8),
            (32, 20),
            (8, 20),
            (20, 32),
            (20, 8),
        ] {
            let mut pixels = vec![0; 40 * 40];
            render(
                &mut pixels,
                40,
                false,
                &[annotation(vec![vec![(20, 20), end]])],
            );
            // Each sample is an exact integer point along the sparse segment.
            for step in 0..=6 {
                let x = 20 + (end.0 - 20) * step / 6;
                let y = 20 + (end.1 - 20) * step / 6;
                assert_eq!(pixels[y as usize * 40 + x as usize], INK);
            }
            let mut reversed = vec![0; 40 * 40];
            render(
                &mut reversed,
                40,
                false,
                &[annotation(vec![vec![end, (20, 20)]])],
            );
            for step in 0..=6 {
                let x = 20 + (end.0 - 20) * step / 6;
                let y = 20 + (end.1 - 20) * step / 6;
                assert_eq!(reversed[y as usize * 40 + x as usize], INK);
            }
        }
    }

    #[test]
    fn adjacent_segments_connect_but_distinct_paths_do_not() {
        let mut pixels = vec![0; 40 * 40];
        render(
            &mut pixels,
            40,
            false,
            &[annotation(vec![
                vec![(5, 5), (15, 5), (15, 15)],
                vec![],
                vec![(30, 30)],
            ])],
        );
        for (x, y) in [(5, 5), (10, 5), (15, 10), (15, 15), (30, 30)] {
            assert_eq!(pixels[y * 40 + x], INK);
        }
        assert_eq!(pixels[10 * 40 + 10], 0);
        assert_eq!(pixels[22 * 40 + 22], 0);
    }

    #[test]
    fn thick_arrow_keeps_two_visible_wings_at_the_press_point() {
        use crate::strokes::{Strokes, Tool};
        for width in [5, 15, 31] {
            let mut strokes = Strokes::default();
            strokes.set_thickness(width);
            strokes.select(Tool::Arrow);
            strokes.begin((100, 150));
            strokes.append((400, 150));
            strokes.finish();
            let mut pixels = vec![0; 500 * 320];
            render(&mut pixels, 500, false, strokes.annotations());
            let head = 24 + 3 * width as usize;
            let half = (head as f64 / 2.0).round() as usize;
            assert_eq!(pixels[150 * 500 + 100], INK);
            assert_eq!(pixels[150 * 500 + 400], INK);
            assert_eq!(pixels[(150 + half) * 500 + 100 + head], INK);
            assert_eq!(pixels[(150 - half) * 500 + 100 + head], INK);
            assert_eq!(pixels[(150 + half) * 500 + 400], 0);
            // Head wings extend far beyond the shaft, with space between them.
            assert_eq!(pixels[(150 + width as usize) * 500 + 100 + head], 0);
        }
    }

    #[test]
    fn signed_extremes_and_offscreen_brushes_clip_to_the_surface() {
        let mut pixels = vec![0; 10 * 10];
        render(
            &mut pixels,
            10,
            false,
            &[annotation(vec![
                vec![(i16::MIN as i32, 5), (i16::MAX as i32, 5)],
                vec![(-1, 0)],
                vec![(11, 0)],
                vec![(-20, -20), (-10, -10)],
                vec![(20, 20), (30, 30)],
            ])],
        );
        for y in 0..10 {
            for x in 0..10 {
                let expected = if (3..=7).contains(&y) || (y <= 2 && (x <= 1 || x == 9)) {
                    INK
                } else {
                    0
                };
                assert_eq!(pixels[y * 10 + x], expected, "at ({x}, {y})");
            }
        }
    }

    #[test]
    fn mode_switch_and_clear_have_no_stale_pixels_and_valid_premultiplied_alpha() {
        let paths = [annotation(vec![vec![(10, 15), (30, 30)], vec![(40, 20)]])];
        let mut pixels = vec![0; 80 * 60];
        render(&mut pixels, 80, true, &paths);
        let drawing_pixels = pixels.clone();
        assert_eq!(pixels[0], 0x01000000);
        for pixel in &pixels {
            let alpha = pixel >> 24;
            for shift in [0, 8, 16] {
                assert!((pixel >> shift) & 0xff <= alpha);
            }
        }
        render(&mut pixels, 80, false, &paths);
        assert_eq!(pixels[0], 0);
        for (&drawing_pixel, &pixel) in drawing_pixels.iter().zip(&pixels) {
            assert_eq!(drawing_pixel == INK, pixel == INK);
        }
        render(&mut pixels, 80, true, &paths);
        assert_eq!(pixels, drawing_pixels);
        render(&mut pixels, 80, true, &[]);
        assert!(pixels.iter().all(|&p| p == 0x01000000));
        render(&mut pixels, 80, false, &[]);
        assert!(pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn mixed_styles_keep_their_color_width_and_paint_order() {
        let annotations = [
            Annotation {
                paths: vec![vec![(5, 8), (15, 8)]],
                style: Style {
                    color: 0xffef4444,
                    thickness: 7,
                },
            },
            Annotation {
                paths: vec![vec![(10, 3), (10, 13)]],
                style: Style {
                    color: 0xff3b82f6,
                    thickness: 1,
                },
            },
        ];
        let mut pixels = vec![0; 21 * 17];
        for drawing in [false, true, false] {
            render(&mut pixels, 21, drawing, &annotations);
            for y in 0..17 {
                for x in 0..21 {
                    let expected = if x == 10 && (3..=13).contains(&y) {
                        0xff3b82f6
                    } else if (2..=18).contains(&x) && (5..=11).contains(&y) {
                        0xffef4444
                    } else if drawing {
                        0x01000000
                    } else {
                        0
                    };
                    assert_eq!(pixels[y * 21 + x], expected, "at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn arrow_branches_and_annotations_are_not_joined() {
        let annotations = [
            Annotation {
                paths: vec![
                    vec![(2, 10), (18, 10)],
                    vec![(18, 10), (12, 4)],
                    vec![],
                    vec![(12, 16), (18, 10)],
                ],
                style: Style {
                    color: 0xff22c55e,
                    thickness: 1,
                },
            },
            annotation(vec![vec![(30, 10)]]),
        ];
        let mut pixels = vec![0; 40 * 21];
        render(&mut pixels, 40, false, &annotations);
        for (x, y) in [
            (2, 10),
            (18, 10),
            (15, 10),
            (12, 4),
            (15, 7),
            (12, 16),
            (15, 13),
        ] {
            assert_eq!(pixels[y * 40 + x], 0xff22c55e);
        }
        for (x, y) in [(12, 7), (12, 13), (15, 16), (22, 10)] {
            assert_eq!(pixels[y * 40 + x], 0);
        }
        assert_eq!(pixels[10 * 40 + 30], INK);
    }

    #[test]
    fn public_styles_are_bounded_odd_and_opaque() {
        for (thickness, expected_width) in [
            (0, 1),
            (1, 1),
            (2, 3),
            (4, 5),
            (30, 31),
            (31, 31),
            (u32::MAX, 31),
        ] {
            for color in [0x00123456, 0x7ffedcba, 0xffffffff] {
                let mut pixels = vec![0; 35 * 35];
                render(
                    &mut pixels,
                    35,
                    false,
                    &[Annotation {
                        paths: vec![vec![(17, 17)]],
                        style: Style { color, thickness },
                    }],
                );
                let radius = expected_width / 2;
                for y in 0..35 {
                    for x in 0..35 {
                        let expected = if (17 - radius..=17 + radius).contains(&x)
                            && (17 - radius..=17 + radius).contains(&y)
                        {
                            color | 0xff000000
                        } else {
                            0
                        };
                        assert_eq!(pixels[y * 35 + x], expected);
                    }
                }
            }
        }
    }

    #[test]
    fn full_i32_extremes_clip_without_overflow_or_offscreen_traversal() {
        for (start, end, axis) in [
            ((i32::MIN, 5), (i32::MAX, 5), 0),
            ((5, i32::MAX), (5, i32::MIN), 1),
            ((i32::MIN, i32::MIN), (i32::MAX, i32::MAX), 2),
            ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN), 2),
            ((i32::MIN, i32::MAX), (i32::MAX, i32::MIN), 3),
            ((i32::MAX, i32::MIN), (i32::MIN, i32::MAX), 3),
            ((i32::MIN, i32::MIN), (i32::MIN, i32::MIN), 3),
            ((i32::MAX, i32::MAX), (i32::MAX, i32::MAX), 3),
        ] {
            let mut pixels = vec![0; 10 * 10];
            render(
                &mut pixels,
                10,
                false,
                &[Annotation {
                    paths: vec![vec![start, end]],
                    style: Style {
                        color: INK,
                        thickness: 1,
                    },
                }],
            );
            for y in 0..10 {
                for x in 0..10 {
                    let ink = match axis {
                        0 => y == 5,
                        1 => x == 5,
                        2 => x == y,
                        _ => false,
                    };
                    assert_eq!(pixels[y * 10 + x], if ink { INK } else { 0 });
                }
            }
        }
    }

    #[test]
    fn bounded_steps_match_bresenham_including_clipped_ties() {
        let coordinates = [-6, -1, 0, 2, 4, 5, 10];
        for x in coordinates {
            for y in coordinates {
                for end_x in coordinates {
                    for end_y in coordinates {
                        for radius in [0, 2, 15] {
                            let mut actual = vec![0; 5 * 5];
                            line(&mut actual, 5, (x, y), (end_x, end_y), radius, INK);
                            let mut expected = vec![0; 5 * 5];
                            let (mut px, mut py) = (x, y);
                            let (dx, dy) = ((end_x - x).abs(), -(end_y - y).abs());
                            let (sx, sy) = (
                                if x < end_x { 1 } else { -1 },
                                if y < end_y { 1 } else { -1 },
                            );
                            let mut error = dx + dy;
                            loop {
                                for by in py - radius as i32..=py + radius as i32 {
                                    for bx in px - radius as i32..=px + radius as i32 {
                                        if (0..5).contains(&bx) && (0..5).contains(&by) {
                                            expected[by as usize * 5 + bx as usize] = INK;
                                        }
                                    }
                                }
                                if (px, py) == (end_x, end_y) {
                                    break;
                                }
                                let twice_error = 2 * error;
                                if twice_error >= dy {
                                    error += dy;
                                    px += sx;
                                }
                                if twice_error <= dx {
                                    error += dx;
                                    py += sy;
                                }
                            }
                            assert_eq!(
                                actual, expected,
                                "{x},{y} to {end_x},{end_y}, radius {radius}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_surface_clips_paths_safely() {
        let mut pixels = [0];
        render(
            &mut pixels,
            1,
            false,
            &[annotation(vec![vec![(-1, -1), (1, 1)]])],
        );
        assert_eq!(pixels, [INK]);
        render(&mut pixels, 1, false, &[]);
        assert_eq!(pixels, [0]);
    }
}
