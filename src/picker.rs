//! Pure color and disk math. Disk coordinates are relative to its center, with y
//! increasing downward. Non-finite HSV components become zero; finite hue wraps
//! and saturation/value clamp. Non-finite adjustment deltas are ignored.
//! Invalid geometry (non-finite coordinates/radius or radius <= 0) leaves the
//! normalized selection unchanged and has no pixel. Alpha is always opaque.

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Hsv {
    pub hue: f64,
    pub saturation: f64,
    pub value: f64,
}

impl Hsv {
    pub fn from_argb(argb: u32) -> Self {
        let r = f64::from((argb >> 16) & 0xff) / 255.0;
        let g = f64::from((argb >> 8) & 0xff) / 255.0;
        let b = f64::from(argb & 0xff) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let chroma = max - min;
        let hue = if chroma == 0.0 {
            0.0
        } else if max == r {
            60.0 * ((g - b) / chroma)
        } else if max == g {
            60.0 * ((b - r) / chroma + 2.0)
        } else {
            60.0 * ((r - g) / chroma + 4.0)
        };
        Self {
            hue: wrap_hue(hue),
            saturation: if max == 0.0 { 0.0 } else { chroma / max },
            value: max,
        }
    }

    pub fn argb(self) -> u32 {
        let hsv = self.normalized();
        let chroma = hsv.value * hsv.saturation;
        let sector = hsv.hue / 60.0;
        let x = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
        let (r, g, b) = match sector as u32 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = hsv.value - chroma;
        let byte = |channel: f64| ((channel + m) * 255.0).round() as u32;
        0xff000000 | (byte(r) << 16) | (byte(g) << 8) | byte(b)
    }

    /// Selects hue/saturation, clamping outside points to the rim. The center
    /// keeps the previous hue and value, while setting saturation to zero.
    pub fn pick_disk(&mut self, x: f64, y: f64, radius: f64) {
        *self = self.normalized();
        if !valid_geometry(x, y, radius) {
            return;
        }
        self.saturation = (x.hypot(y) / radius).min(1.0);
        if x != 0.0 || y != 0.0 {
            self.hue = wrap_hue(180.0 - y.atan2(x).to_degrees());
        }
    }

    pub fn adjust(&mut self, hue_delta: f64, saturation_delta: f64, value_delta: f64) {
        *self = self.normalized();
        self.hue = wrap_hue(self.hue + wrap_hue(hue_delta));
        // Clamp deltas before addition so even finite f64 extremes cannot overflow.
        self.saturation = unit(self.saturation + finite(saturation_delta).clamp(-1.0, 1.0));
        self.value = unit(self.value + finite(value_delta).clamp(-1.0, 1.0));
    }

    fn normalized(self) -> Self {
        Self {
            hue: wrap_hue(self.hue),
            saturation: unit(self.saturation),
            value: unit(self.value),
        }
    }
}

/// Accepts exactly six ASCII hex digits, optionally prefixed by '#', after trim.
pub fn parse_hex(text: &str) -> Option<u32> {
    let text = text.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(digits, 16)
        .ok()
        .map(|rgb| 0xff000000 | rgb)
}

pub fn hex(argb: u32) -> String {
    format!("#{:06X}", argb & 0xffffff)
}

/// Returns an opaque wheel pixel, including the rim; outside/invalid points
/// return None. Value follows the same normalization as Hsv.
pub fn disk_pixel(x: f64, y: f64, radius: f64, value: f64) -> Option<u32> {
    if !valid_geometry(x, y, radius) || x.hypot(y) > radius {
        return None;
    }
    let mut hsv = Hsv {
        hue: 0.0,
        saturation: 0.0,
        value,
    };
    hsv.pick_disk(x, y, radius);
    Some(hsv.argb())
}

fn valid_geometry(x: f64, y: f64, radius: f64) -> bool {
    x.is_finite() && y.is_finite() && radius.is_finite() && radius > 0.0
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn unit(value: f64) -> f64 {
    finite(value).clamp(0.0, 1.0)
}

fn wrap_hue(hue: f64) -> f64 {
    let hue = finite(hue).rem_euclid(360.0);
    // rem_euclid can round a tiny negative hue up to exactly 360.
    if hue >= 360.0 { 0.0 } else { hue }
}

#[cfg(test)]
mod tests {
    use super::{Hsv, disk_pixel, hex, parse_hex};

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
    }

    fn bounded(hsv: Hsv) {
        assert!(hsv.hue.is_finite() && (0.0..360.0).contains(&hsv.hue));
        assert!(hsv.saturation.is_finite() && (0.0..=1.0).contains(&hsv.saturation));
        assert!(hsv.value.is_finite() && (0.0..=1.0).contains(&hsv.value));
        assert_eq!(hsv.argb() >> 24, 0xff);
    }

    #[test]
    fn basic_colors_and_rounding() {
        for (argb, hue, saturation, value) in [
            (0xffff0000, 0.0, 1.0, 1.0),
            (0xffffff00, 60.0, 1.0, 1.0),
            (0xff00ff00, 120.0, 1.0, 1.0),
            (0xff00ffff, 180.0, 1.0, 1.0),
            (0xff0000ff, 240.0, 1.0, 1.0),
            (0xffff00ff, 300.0, 1.0, 1.0),
            (0xff000000, 0.0, 0.0, 0.0),
            (0xffffffff, 0.0, 0.0, 1.0),
            (0xff808080, 0.0, 0.0, 128.0 / 255.0),
        ] {
            let hsv = Hsv::from_argb(argb);
            close(hsv.hue, hue);
            close(hsv.saturation, saturation);
            close(hsv.value, value);
            assert_eq!(hsv.argb(), argb);
            bounded(hsv);
        }
        assert_eq!(Hsv::from_argb(0x00123456).argb(), 0xff123456);
        assert_eq!(
            Hsv {
                hue: 0.0,
                saturation: 0.0,
                value: 0.5
            }
            .argb(),
            0xff808080
        );
        assert_eq!(
            Hsv {
                hue: 0.0,
                saturation: 1.0,
                value: 0.5
            }
            .argb(),
            0xff800000
        );
    }

    #[test]
    fn arbitrary_rgb_roundtrips() {
        let mut color = 0x12345678_u32;
        for _ in 0..16384 {
            color = color.wrapping_mul(1664525).wrapping_add(1013904223);
            let hsv = Hsv::from_argb(color);
            bounded(hsv);
            let result = hsv.argb();
            for shift in [0, 8, 16] {
                let original = ((color >> shift) & 0xff) as i32;
                let converted = ((result >> shift) & 0xff) as i32;
                assert!(
                    (original - converted).abs() <= 1,
                    "{color:08X} -> {result:08X}"
                );
            }
        }
    }

    #[test]
    fn hex_format_and_strict_parsing() {
        for text in ["#123AbC", "123abc", "  #123ABC\r\n", "\t123ABC "] {
            assert_eq!(parse_hex(text), Some(0xff123abc));
        }
        for text in [
            "",
            "#",
            "#fff",
            "fff",
            "#12345678",
            "12345678",
            "-12345",
            "+12345",
            "0x123456",
            "##123456",
            "# 123456",
            "12 456",
            "12345g",
            "１２３４５６",
            "123456\0",
            "#123456 junk",
        ] {
            assert_eq!(parse_hex(text), None, "{text:?}");
        }
        assert_eq!(hex(0x0012abcd), "#12ABCD");
        assert_eq!(hex(0xff000000), "#000000");
        assert_eq!(hex(0x12ffffff), "#FFFFFF");
        for color in [0x00000000, 0x12345678, 0xffffffff, 0xfff97316] {
            assert_eq!(parse_hex(&hex(color)), Some(color | 0xff000000));
        }
    }

    #[test]
    fn disk_orientation_center_and_rim() {
        let mut hsv = Hsv {
            hue: 37.0,
            saturation: 1.0,
            value: 0.5,
        };
        hsv.pick_disk(0.0, -0.0, 10.0);
        assert_eq!(
            hsv,
            Hsv {
                hue: 37.0,
                saturation: 0.0,
                value: 0.5
            }
        );
        for (x, y, hue) in [
            (-10.0, 0.0, 0.0),
            (0.0, 10.0, 90.0),
            (10.0, 0.0, 180.0),
            (0.0, -10.0, 270.0),
        ] {
            hsv.pick_disk(x, y, 10.0);
            close(hsv.hue, hue);
            assert_eq!(hsv.saturation, 1.0);
            assert_eq!(hsv.value, 0.5);
            assert_eq!(disk_pixel(x, y, 10.0, hsv.value), Some(hsv.argb()));
            hsv.pick_disk(x * 100.0, y * 100.0, 10.0);
            close(hsv.hue, hue);
            assert_eq!(hsv.saturation, 1.0);
            assert_eq!(disk_pixel(x * 1.001, y * 1.001, 10.0, 1.0), None);
        }
        hsv.pick_disk(3.0, 4.0, 10.0);
        close(hsv.saturation, 0.5);
        assert_eq!(disk_pixel(0.0, 0.0, 10.0, 1.0), Some(0xffffffff));
        assert_eq!(disk_pixel(0.0, 0.0, 10.0, 0.0), Some(0xff000000));
        assert_eq!(disk_pixel(-10.0, 0.0, 10.0, 1.0), Some(0xffff0000));
    }

    #[test]
    fn wheel_marker_points_select_the_same_candidate() {
        for hue in (0..360).step_by(3) {
            for saturation in [0.0, 0.25, 0.5, 0.95, 1.0] {
                let original = Hsv {
                    hue: f64::from(hue),
                    saturation,
                    value: 0.73,
                };
                let angle = (180.0 - original.hue).to_radians();
                let radius = 100.0;
                let x = radius * saturation * angle.cos();
                let y = radius * saturation * angle.sin();
                let mut selected = original;
                selected.pick_disk(x, y, radius);
                bounded(selected);
                close(selected.saturation, saturation);
                close(selected.hue, original.hue);
                assert_eq!(selected.argb(), original.argb());
                // Interior pixels and selection use identical math. At the rim,
                // trig rounding may place the marker just outside the exact disk.
                if saturation < 1.0 {
                    assert_eq!(
                        disk_pixel(x, y, radius, original.value),
                        Some(selected.argb())
                    );
                }
            }
        }
    }

    #[test]
    fn keyboard_deltas_wrap_and_clamp() {
        let mut hsv = Hsv {
            hue: 359.0,
            saturation: 0.99,
            value: 0.01,
        };
        hsv.adjust(3.0, 0.02, -0.02);
        assert_eq!(
            hsv,
            Hsv {
                hue: 2.0,
                saturation: 1.0,
                value: 0.0
            }
        );
        hsv.adjust(-3.0, -0.02, 0.02);
        assert_eq!(
            hsv,
            Hsv {
                hue: 359.0,
                saturation: 0.98,
                value: 0.02
            }
        );
        hsv.adjust(720.0, -10.0, 10.0);
        assert_eq!(
            hsv,
            Hsv {
                hue: 359.0,
                saturation: 0.0,
                value: 1.0
            }
        );
        hsv.adjust(f64::NAN, f64::INFINITY, f64::NEG_INFINITY);
        assert_eq!(
            hsv,
            Hsv {
                hue: 359.0,
                saturation: 0.0,
                value: 1.0
            }
        );
        hsv.adjust(f64::MAX, f64::MAX, -f64::MAX);
        bounded(hsv);
        assert_eq!(hsv.saturation, 1.0);
        assert_eq!(hsv.value, 0.0);
    }

    #[test]
    fn invalid_and_extreme_inputs_are_deterministic() {
        let valid = Hsv {
            hue: 123.0,
            saturation: 0.4,
            value: 0.7,
        };
        for (x, y, radius) in [
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            (0.0, 0.0, f64::NAN),
            (0.0, 0.0, f64::INFINITY),
            (f64::NAN, 1.0, 1.0),
            (1.0, f64::INFINITY, 1.0),
            (f64::NEG_INFINITY, 0.0, 1.0),
        ] {
            let mut hsv = valid;
            hsv.pick_disk(x, y, radius);
            assert_eq!(hsv, valid);
            assert_eq!(disk_pixel(x, y, radius, 1.0), None);
        }
        for component in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -f64::MAX,
            -1.0,
            -f64::MIN_POSITIVE,
            0.0,
            1.0,
            360.0,
            f64::MAX,
        ] {
            let invalid = Hsv {
                hue: component,
                saturation: component,
                value: component,
            };
            let color = invalid.argb();
            assert_eq!(color >> 24, 0xff);
            let mut adjusted = invalid;
            adjusted.adjust(0.0, 0.0, 0.0);
            bounded(adjusted);
            assert_eq!(adjusted.argb(), color);
            let mut picked = invalid;
            picked.pick_disk(0.0, 0.0, 0.0);
            assert_eq!(picked, adjusted);
        }
        let mut hsv = valid;
        hsv.pick_disk(f64::MAX, f64::MAX, f64::MAX);
        bounded(hsv);
        close(hsv.hue, 135.0);
        assert_eq!(hsv.saturation, 1.0);
        assert_eq!(disk_pixel(f64::MAX, f64::MAX, f64::MAX, 1.0), None);
        hsv.pick_disk(f64::MIN_POSITIVE, 0.0, f64::MAX);
        bounded(hsv);
        hsv.pick_disk(1.0, 1.0, f64::MIN_POSITIVE);
        bounded(hsv);
        assert_eq!(hsv.saturation, 1.0);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert_eq!(disk_pixel(0.0, 0.0, 1.0, value), Some(0xff000000));
        }
        assert_eq!(disk_pixel(0.0, 0.0, 1.0, f64::MAX), Some(0xffffffff));
    }
}
