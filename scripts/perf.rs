//! Offscreen release-mode measurement. No windows, input injection or DIB upload.
#[path = "../src/drawing.rs"]
mod drawing;
#[path = "../src/strokes.rs"]
mod strokes;
#[path = "../src/trail.rs"]
mod trail;

use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use strokes::{Point, Strokes, Style};
use trail::Trail;

fn points(width: usize, height: usize, count: usize, offset: usize) -> Vec<Point> {
    (0..count)
        .map(|i| {
            let angle = i as f64 / (count - 1) as f64 * std::f64::consts::TAU * 1.5;
            (
                (width as f64 * 0.5 + angle.cos() * width as f64 * 0.35 + offset as f64 * 2.0)
                    as i32,
                (height as f64 * 0.5 + angle.sin() * height as f64 * 0.3 + offset as f64 * 2.0)
                    as i32,
            )
        })
        .collect()
}

fn measure(label: &str, mut work: impl FnMut()) {
    for _ in 0..2 {
        work();
    }
    let mut samples = Vec::new();
    for _ in 0..9 {
        let start = Instant::now();
        work();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    println!(
        "{label},{:.3},{:.3},{:.3}",
        samples[0], samples[4], samples[8]
    );
}

fn main() {
    println!("case,min_ms,median_ms,max_ms");
    for (width, height) in [(1920, 1200), (5760, 1200)] {
        for (count, thickness) in [(24, 5), (240, 21)] {
            let input = points(width, height, count, 0);
            measure(
                &format!("{width}x{height}_input_{count}_{thickness}"),
                || {
                    let mut scene = Strokes::default();
                    scene.set_thickness(thickness);
                    scene.begin(input[0]);
                    for &point in &input[1..] {
                        scene.append(point);
                    }
                    scene.finish();
                    black_box(scene);
                },
            );
            let mut scene = Strokes::default();
            scene.set_thickness(thickness);
            for offset in 0..4 {
                let path = points(width, height, count, offset);
                scene.begin(path[0]);
                for &point in &path[1..] {
                    scene.append(point);
                }
                scene.finish();
            }
            let mut trail = Trail::default();
            trail.begin(
                input[0],
                Style {
                    thickness,
                    ..Style::default()
                },
                Duration::ZERO,
            );
            for (i, &point) in input.iter().enumerate().skip(1) {
                trail.append(
                    point,
                    Duration::from_micros(i as u64 * 950_000 / (count - 1) as u64),
                );
            }
            trail.finish();
            let mut pixels = vec![0; width * height];
            measure(
                &format!("{width}x{height}_permanent_{count}_{thickness}"),
                || {
                    drawing::render(&mut pixels, width, false, scene.annotations());
                    black_box(&pixels);
                },
            );
            measure(
                &format!("{width}x{height}_fade_{count}_{thickness}"),
                || {
                    drawing::render(&mut pixels, width, false, scene.annotations());
                    drawing::paint_trail(
                        &mut pixels,
                        width,
                        trail.paint_parts(Duration::from_millis(975)),
                    );
                    black_box(&pixels);
                },
            );
        }
    }
}
