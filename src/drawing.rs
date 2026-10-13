const INK: u32 = 0xff_f9_73_16; // Premultiplied BGRA in a little-endian DIB.
const RADIUS: i32 = 2;

pub fn render(pixels: &mut [u32], width: usize, capture: bool, cleared: bool) {
    assert!(width > 0 && pixels.len().is_multiple_of(width));
    let height = pixels.len() / width;
    // ponytail: full-screen CPU fill/upload; measure before switching to a GPU renderer.
    pixels.fill(if capture { 0x01_00_00_00 } else { 0 });
    if cleared {
        return;
    }
    let w = width as i32;
    let h = height as i32;
    line(pixels, width, (w / 8, h / 4), (3 * w / 8, h / 2));
    let (left, top, right, bottom) = (w / 2, h / 3, 3 * w / 4, 2 * h / 3);
    for (start, end) in [
        ((left, top), (right, top)),
        ((right, top), (right, bottom)),
        ((right, bottom), (left, bottom)),
        ((left, bottom), (left, top)),
    ] {
        line(pixels, width, start, end);
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
    fn automatic_shapes_leave_background_and_rectangle_interior_transparent() {
        let mut pixels = vec![0; 80 * 60];
        render(&mut pixels, 80, false, false);
        for (x, y) in [(10, 15), (20, 22), (30, 30), (40, 20), (60, 40)] {
            assert_eq!(pixels[y * 80 + x], INK);
        }
        assert_eq!(pixels[0], 0);
        assert_eq!(pixels[30 * 80 + 50], 0);
    }

    #[test]
    fn mode_switch_and_clear_have_no_stale_pixels_and_valid_premultiplied_alpha() {
        let mut pixels = vec![0; 80 * 60];
        render(&mut pixels, 80, true, false);
        assert_eq!(pixels[0], 0x01000000);
        for pixel in &pixels {
            let alpha = pixel >> 24;
            for shift in [0, 8, 16] {
                assert!((pixel >> shift) & 0xff <= alpha);
            }
        }
        render(&mut pixels, 80, false, false);
        assert_eq!(pixels[0], 0);
        render(&mut pixels, 80, true, true);
        assert!(pixels.iter().all(|&p| p == 0x01000000));
        render(&mut pixels, 80, false, true);
        assert!(pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn tiny_surface_clips_shapes_safely() {
        render(&mut [0], 1, false, false);
    }
}
