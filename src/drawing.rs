const INK: u32 = 0xff_f9_73_16; // Premultiplied BGRA in a little-endian DIB.
const RADIUS: i32 = 2;

pub fn render(pixels: &mut [u32], width: usize, drawing: bool, paths: &[Vec<(i32, i32)>]) {
    assert!(width > 0 && pixels.len().is_multiple_of(width));
    // ponytail: full-screen fill/upload and O(total stroke points) replay per change;
    // measure at the tested resolution before switching to a GPU renderer.
    pixels.fill(if drawing { 0x01_00_00_00 } else { 0 });
    for path in paths {
        if let Some(&point) = path.first() {
            line(pixels, width, point, point);
        }
        for segment in path.windows(2) {
            line(pixels, width, segment[0], segment[1]);
        }
    }
}

fn line(pixels: &mut [u32], width: usize, start: (i32, i32), end: (i32, i32)) {
    let (mut x, mut y) = start;
    let (dx, dy) = ((end.0 - x).abs(), -(end.1 - y).abs());
    let (sx, sy) = (
        if x < end.0 { 1 } else { -1 },
        if y < end.1 { 1 } else { -1 },
    );
    let mut error = dx + dy;
    loop {
        for py in y - RADIUS..=y + RADIUS {
            for px in x - RADIUS..=x + RADIUS {
                if px >= 0 && px < width as i32 && py >= 0 && py < (pixels.len() / width) as i32 {
                    pixels[py as usize * width + px as usize] = INK;
                }
            }
        }
        if (x, y) == end {
            break;
        }
        let twice_error = 2 * error;
        if twice_error >= dy {
            error += dy;
            x += sx;
        }
        if twice_error <= dx {
            error += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        render(&mut pixels, 11, false, &[vec![(5, 5)]]);
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
            render(&mut pixels, 40, false, &[vec![(20, 20), end]]);
            // Each sample is an exact integer point along the sparse segment.
            for step in 0..=6 {
                let x = 20 + (end.0 - 20) * step / 6;
                let y = 20 + (end.1 - 20) * step / 6;
                assert_eq!(pixels[y as usize * 40 + x as usize], INK);
            }
            let mut reversed = vec![0; 40 * 40];
            render(&mut reversed, 40, false, &[vec![end, (20, 20)]]);
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
            &[vec![(5, 5), (15, 5), (15, 15)], vec![], vec![(30, 30)]],
        );
        for (x, y) in [(5, 5), (10, 5), (15, 10), (15, 15), (30, 30)] {
            assert_eq!(pixels[y * 40 + x], INK);
        }
        assert_eq!(pixels[10 * 40 + 10], 0);
        assert_eq!(pixels[22 * 40 + 22], 0);
    }

    #[test]
    fn signed_extremes_and_offscreen_brushes_clip_to_the_surface() {
        let mut pixels = vec![0; 10 * 10];
        render(
            &mut pixels,
            10,
            false,
            &[
                vec![(i16::MIN as i32, 5), (i16::MAX as i32, 5)],
                vec![(-1, 0)],
                vec![(11, 0)],
                vec![(-20, -20), (-10, -10)],
                vec![(20, 20), (30, 30)],
            ],
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
        let paths = [vec![(10, 15), (30, 30)], vec![(40, 20)]];
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
    fn tiny_surface_clips_paths_safely() {
        let mut pixels = [0];
        render(&mut pixels, 1, false, &[vec![(-1, -1), (1, 1)]]);
        assert_eq!(pixels, [INK]);
        render(&mut pixels, 1, false, &[]);
        assert_eq!(pixels, [0]);
    }
}
