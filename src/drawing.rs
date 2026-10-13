use crate::strokes::{Annotation, Point, Style};

const OFFSETS: [f64; 4] = [-0.375, -0.125, 0.125, 0.375];

pub fn render<'a>(
    pixels: &mut [u32],
    width: usize,
    drawing: bool,
    annotations: impl IntoIterator<Item = &'a Annotation>,
) {
    assert!(width > 0 && pixels.len().is_multiple_of(width));
    pixels.fill(if drawing { 0x01_00_00_00 } else { 0 });
    if pixels.is_empty() {
        return;
    }
    let mut samples = vec![0; width * 16];
    let mut segments = Vec::new();
    for annotation in annotations {
        segments.clear();
        append_segments(&mut segments, annotation, 255);
        paint_group(pixels, width, &mut samples, &segments, annotation.style);
    }
}

/// Paint consecutive parts of each gesture over the existing premultiplied BGRA bitmap.
/// The first part supplies the group's frozen style; opacity is linear, supplied by the caller.
pub fn paint_trail<'a>(
    pixels: &mut [u32],
    width: usize,
    parts: impl IntoIterator<Item = (u64, &'a Annotation, u8)>,
) {
    assert!(width > 0 && pixels.len().is_multiple_of(width));
    if pixels.is_empty() {
        return;
    }
    let mut samples = vec![0; width * 16];
    let mut segments = Vec::new();
    let mut parts = parts.into_iter().peekable();
    while let Some((gesture, annotation, opacity)) = parts.next() {
        segments.clear();
        append_segments(&mut segments, annotation, opacity);
        while parts.peek().is_some_and(|part| part.0 == gesture) {
            let (_, annotation, opacity) = parts.next().unwrap();
            append_segments(&mut segments, annotation, opacity);
        }
        paint_group(pixels, width, &mut samples, &segments, annotation.style);
    }
}

struct Segment {
    start: (f64, f64),
    delta: (f64, f64),
    length_squared: f64,
    opacity: u8,
}

fn append_segments(segments: &mut Vec<Segment>, annotation: &Annotation, opacity: u8) {
    if opacity == 0 {
        return;
    }
    let mut append = |start: Point, end: Point| {
        let start = (f64::from(start.0), f64::from(start.1));
        let delta = (f64::from(end.0) - start.0, f64::from(end.1) - start.1);
        segments.push(Segment {
            start,
            delta,
            length_squared: delta.0 * delta.0 + delta.1 * delta.1,
            opacity,
        });
    };
    for path in &annotation.paths {
        if path.len() == 1 {
            append(path[0], path[0]);
        }
        for pair in path.windows(2) {
            append(pair[0], pair[1]);
        }
    }
}

impl Segment {
    fn distance_squared(&self, x: f64, y: f64) -> f64 {
        let (px, py) = (x - self.start.0, y - self.start.1);
        let t = if self.length_squared == 0.0 {
            0.0
        } else {
            ((px * self.delta.0 + py * self.delta.1) / self.length_squared).clamp(0.0, 1.0)
        };
        (px - t * self.delta.0).powi(2) + (py - t * self.delta.1).powi(2)
    }

    fn x_range(&self, y: f64, reach: f64, width: usize) -> std::ops::Range<usize> {
        let (mut first, mut last) = (0.0_f64, 1.0_f64);
        if self.delta.1 == 0.0 {
            if (y - self.start.1).abs() > reach {
                return 0..0;
            }
        } else {
            let a = (y - reach - self.start.1) / self.delta.1;
            let b = (y + reach - self.start.1) / self.delta.1;
            first = first.max(a.min(b));
            last = last.min(a.max(b));
            if first > last {
                return 0..0;
            }
        }
        let a = self.start.0 + first * self.delta.0;
        let b = self.start.0 + last * self.delta.0;
        // Expanded scanline slab bounds work along a diagonal, not its whole bounding box.
        let left = (a.min(b) - reach).ceil().clamp(0.0, width as f64) as usize;
        let right = (a.max(b) + reach + 1.0).floor().clamp(0.0, width as f64) as usize;
        left..right
    }
}

fn paint_group(
    pixels: &mut [u32],
    width: usize,
    samples: &mut [u8],
    segments: &[Segment],
    style: Style,
) {
    // Public styles may bypass the model's validation. Round even widths up.
    let radius = f64::from(style.thickness.clamp(1, 31) | 1) / 2.0;
    let reach = radius + OFFSETS[3];
    let height = pixels.len() / width;
    let (mut top, mut bottom) = (height, 0);
    for segment in segments {
        let (x, y) = segment.start;
        let (end_x, end_y) = (x + segment.delta.0, y + segment.delta.1);
        if x.max(end_x) + reach < 0.0 || x.min(end_x) - reach > (width - 1) as f64 {
            continue;
        }
        top = top.min((y.min(end_y) - reach).ceil().clamp(0.0, height as f64) as usize);
        bottom = bottom.max(
            (y.max(end_y) + reach + 1.0)
                .floor()
                .clamp(0.0, height as f64) as usize,
        );
    }
    // ponytail: O(viewport-clipped row span * group segments), plus covered samples;
    // bin segments by row if long gestures make this scan the replay bottleneck.
    for y in top..bottom {
        samples.fill(0);
        for segment in segments {
            for x in segment.x_range(y as f64, reach, width) {
                for (sy, oy) in OFFSETS.iter().enumerate() {
                    for (sx, ox) in OFFSETS.iter().enumerate() {
                        let sample = &mut samples[x * 16 + sy * 4 + sx];
                        // Use original endpoints, so clipping never introduces artificial caps.
                        if *sample < segment.opacity
                            && segment.distance_squared(x as f64 + ox, y as f64 + oy)
                                <= radius * radius
                        {
                            *sample = segment.opacity;
                        }
                    }
                }
            }
        }
        for (pixel, coverage) in pixels[y * width..(y + 1) * width]
            .iter_mut()
            .zip(samples.chunks_exact(16))
        {
            let alpha = (coverage.iter().map(|&v| u32::from(v)).sum::<u32>() + 8) / 16;
            if alpha != 0 {
                *pixel = source_over(*pixel, style.color, alpha);
            }
        }
    }
}

fn source_over(destination: u32, color: u32, alpha: u32) -> u32 {
    let mut result = 0;
    for shift in [0, 8, 16, 24] {
        let source = if shift == 24 {
            255
        } else {
            (color >> shift) & 255
        };
        let dest = (destination >> shift) & 255;
        let channel = (source * alpha + 127) / 255 + (dest * (255 - alpha) + 127) / 255;
        result |= channel << shift;
    }
    result
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
    fn poc7_circular_dots_and_round_caps() {
        let mut dot = vec![0; 15 * 11];
        render(&mut dot, 15, false, &[annotation(vec![vec![(5, 5)]])]);
        assert_eq!(dot[5 * 15 + 5], INK);
        assert_eq!(dot[5 * 15 + 3], INK);
        assert_eq!(dot[3 * 15 + 3], 0x30_2f_16_04); // 3 of 16 samples, not a square corner.
        assert_eq!(dot[2 * 15 + 5], 0);
        let mut capsule = vec![0; 15 * 11];
        render(
            &mut capsule,
            15,
            false,
            &[annotation(vec![vec![(5, 5), (9, 5)]])],
        );
        for y in 0..11 {
            for x in 0..=5 {
                assert_eq!(capsule[y * 15 + x], dot[y * 15 + x]);
                assert_eq!(capsule[y * 15 + 14 - x], dot[y * 15 + x]);
            }
        }
        assert_eq!(INK.to_le_bytes(), [0x16, 0x73, 0xf9, 0xff]);
    }

    #[test]
    fn poc7_fractional_aa_edges_preserve_nominal_odd_width() {
        let thin = Annotation {
            paths: vec![vec![(5, 5), (9, 5)]],
            style: Style {
                thickness: 1,
                ..Style::default()
            },
        };
        let mut pixels = vec![0; 15 * 11];
        render(&mut pixels, 15, false, &[thin]);
        assert_eq!(pixels[5 * 15 + 7], INK);
        assert_eq!(pixels[5 * 15 + 5] >> 24, 223); // Round cap covers 14 samples.
        assert_eq!(pixels[4 * 15 + 7], 0);
        let mut dot = annotation(vec![vec![(5, 5)]]);
        dot.style.thickness = 1;
        render(&mut pixels, 15, false, &[dot]);
        assert_eq!(pixels[5 * 15 + 5] >> 24, 191); // Radius 0.5, not an opaque square.
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
            assert_eq!(pixels, reversed);
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
        for width in [5, 15, 31] {
            for length in [100, 300] {
                // Renderer-only fixture uses the shared 48+6w, 75%-cap geometry contract.
                // Model geometry is tested by strokes.rs, not this focused renderer check.
                let head = (48.0 + 6.0 * f64::from(width)).min(f64::from(length) * 0.75);
                let half = (head / 2.0).round() as usize;
                let head = head.round() as usize;
                let arrow = Annotation {
                    paths: vec![
                        vec![(100, 150), (100 + length, 150)],
                        vec![(100, 150), ((100 + head) as i32, (150 + half) as i32)],
                        vec![(100, 150), ((100 + head) as i32, (150 - half) as i32)],
                    ],
                    style: Style {
                        thickness: width,
                        ..Style::default()
                    },
                };
                let mut pixels = vec![0; 500 * 320];
                render(&mut pixels, 500, false, &[arrow]);
                assert_eq!(pixels[150 * 500 + 100], INK);
                assert_eq!(pixels[150 * 500 + (100 + length) as usize], INK);
                assert_eq!(pixels[(150 + half) * 500 + 100 + head], INK);
                assert_eq!(pixels[(150 - half) * 500 + 100 + head], INK);
                assert_eq!(pixels[(150 + half + width as usize) * 500 + 100 + head], 0);
                assert_eq!(pixels[(150 + width as usize / 2 + 1) * 500 + 100 + head], 0);
            }
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
        for y in 3..=7 {
            assert!(pixels[y * 10..(y + 1) * 10].iter().all(|&p| p == INK));
        }
        assert_eq!(pixels[0], INK); // Center of the left offscreen dot still reaches the viewport.
        assert_eq!(pixels[9], INK);
        assert!(pixels[2 * 10 + 1] >> 24 > 0 && pixels[2 * 10 + 1] >> 24 < 255);
        assert_eq!(pixels[2 * 10 + 5], 0);
        assert!(pixels[8 * 10..].iter().all(|&p| p == 0));
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
            for y in 4..=12 {
                assert_eq!(pixels[y * 21 + 10], 0xff3b82f6);
            }
            for (x, y) in [(5, 8), (15, 8), (7, 5), (13, 11)] {
                assert_eq!(pixels[y * 21 + x], 0xffef4444);
            }
            assert_eq!(pixels[4 * 21 + 7], if drawing { 0x01000000 } else { 0 });
            assert_eq!(pixels[5 * 21 + 2], if drawing { 0x01000000 } else { 0 });
            let mut reversed = vec![0; 21 * 17];
            render(&mut reversed, 21, drawing, annotations.iter().rev());
            assert_eq!(reversed[8 * 21 + 10], 0xffef4444);
            assert_ne!(pixels, reversed);
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
            let pixel = pixels[y * 40 + x];
            assert!(pixel >> 24 >= 191, "visible branch sample at ({x}, {y})");
            assert_eq!(pixel, source_over(0, 0xff22c55e, pixel >> 24));
        }
        for (x, y) in [(12, 7), (12, 13), (15, 16), (22, 10)] {
            assert_eq!(pixels[y * 40 + x], 0);
        }
        assert_eq!(pixels[10 * 40 + 30], INK);
    }

    #[test]
    fn public_styles_are_bounded_odd_with_opaque_color_and_aa_coverage() {
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
                let mut normalized = vec![0; 35 * 35];
                render(
                    &mut normalized,
                    35,
                    false,
                    &[Annotation {
                        paths: vec![vec![(17, 17)]],
                        style: Style {
                            color: color | 0xff000000,
                            thickness: expected_width,
                        },
                    }],
                );
                assert_eq!(pixels, normalized);
                let radius = expected_width as usize / 2;
                assert_eq!(
                    pixels[17 * 35 + 17] >> 24,
                    if expected_width == 1 { 191 } else { 255 }
                );
                assert!(pixels[17 * 35 + 17 + radius] >> 24 > 0);
                assert_eq!(pixels[17 * 35 + 18 + radius], 0);
                for pixel in pixels {
                    let alpha = pixel >> 24;
                    assert_eq!(pixel, source_over(0, color | 0xff000000, alpha));
                }
            }
        }
    }

    #[test]
    fn poc7_full_i32_extremes_clip_without_overflow_or_offscreen_traversal() {
        for (start, end, visible) in [
            ((i32::MIN, 5), (i32::MAX, 5), vec![(-30, 5), (30, 5)]),
            ((5, i32::MAX), (5, i32::MIN), vec![(5, 30), (5, -30)]),
            (
                (i32::MIN, i32::MIN),
                (i32::MAX, i32::MAX),
                vec![(-30, -30), (30, 30)],
            ),
            (
                (i32::MAX, i32::MAX),
                (i32::MIN, i32::MIN),
                vec![(30, 30), (-30, -30)],
            ),
            (
                (i32::MIN, i32::MAX),
                (i32::MAX, i32::MIN),
                vec![(-30, 29), (29, -30)],
            ),
            (
                (i32::MAX, i32::MIN),
                (i32::MIN, i32::MAX),
                vec![(29, -30), (-30, 29)],
            ),
            ((i32::MIN, i32::MIN), (i32::MIN, i32::MIN), vec![]),
            ((i32::MAX, i32::MAX), (i32::MAX, i32::MAX), vec![]),
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
            let mut expected = vec![0; 10 * 10];
            render(
                &mut expected,
                10,
                false,
                &[Annotation {
                    paths: vec![visible],
                    style: Style {
                        color: INK,
                        thickness: 1,
                    },
                }],
            );
            assert_eq!(pixels, expected, "{start:?} to {end:?}");
        }
    }

    #[test]
    fn bounded_scan_matches_unclipped_capsule_reference() {
        let coordinates = [-6, -1, 0, 2, 4, 5, 10];
        for x in coordinates {
            for y in coordinates {
                for end_x in coordinates {
                    for end_y in coordinates {
                        for thickness in [1, 5, 31] {
                            let mut actual = vec![0; 5 * 5];
                            render(
                                &mut actual,
                                5,
                                false,
                                &[Annotation {
                                    paths: vec![vec![(x, y), (end_x, end_y)]],
                                    style: Style {
                                        color: INK,
                                        thickness,
                                    },
                                }],
                            );
                            let mut expected = vec![0; 5 * 5];
                            let (dx, dy) = (f64::from(end_x - x), f64::from(end_y - y));
                            let length = dx.hypot(dy);
                            for py in 0..5 {
                                for px in 0..5 {
                                    let mut covered = 0;
                                    for oy in OFFSETS {
                                        for ox in OFFSETS {
                                            let (rx, ry) = (
                                                px as f64 + ox - f64::from(x),
                                                py as f64 + oy - f64::from(y),
                                            );
                                            let along = if length == 0.0 {
                                                0.0
                                            } else {
                                                (rx * dx + ry * dy) / length
                                            };
                                            let distance = if along <= 0.0 {
                                                rx.hypot(ry)
                                            } else if along >= length {
                                                (rx - dx).hypot(ry - dy)
                                            } else {
                                                (rx * dy - ry * dx).abs() / length
                                            };
                                            covered +=
                                                u32::from(distance <= f64::from(thickness) / 2.0);
                                        }
                                    }
                                    expected[py * 5 + px] =
                                        source_over(0, INK, (covered * 255 + 8) / 16);
                                }
                            }
                            assert_eq!(
                                actual, expected,
                                "{x},{y} to {end_x},{end_y}, thickness {thickness}"
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
