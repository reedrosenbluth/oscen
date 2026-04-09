use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::f32::consts::TAU;

const NUM_SAMPLES: usize = 512;
const WARMUP_CYCLES: usize = 2;
const IMG_WIDTH: u32 = 1452;
const IMG_HEIGHT: u32 = 160;
const LINE_RADIUS: f32 = 1.5;

const BG: [u8; 4] = [0x26, 0x26, 0x26, 0xFF];
const LINE: [u8; 4] = [0x99, 0x99, 0x99, 0xFF];
const CENTER_LINE: [u8; 4] = [0x33, 0x33, 0x33, 0xFF];

pub struct FmWaveformParams {
    pub op3_ratio: f32,
    pub op3_level: f32,
    pub op3_feedback: f32,
    pub op2_ratio: f32,
    pub op2_level: f32,
    pub op2_feedback: f32,
    pub route: f32,
}

fn compute_waveform(params: &FmWaveformParams) -> Vec<f32> {
    let total_samples = NUM_SAMPLES * (WARMUP_CYCLES + 1);
    let mut op3_prev = 0.0f32;
    let mut op2_prev = 0.0f32;
    let mut samples = Vec::with_capacity(NUM_SAMPLES);

    for i in 0..total_samples {
        let phase = (i % NUM_SAMPLES) as f32 / NUM_SAMPLES as f32;

        let op3_total_phase = params.op3_ratio * phase + params.op3_feedback * op3_prev;
        let op3_out = (op3_total_phase * TAU).sin() * params.op3_level;
        op3_prev = op3_out;

        let op3_to_2 = op3_out * (1.0 - params.route);
        let op3_to_1 = op3_out * params.route;

        let op2_total_phase =
            params.op2_ratio * phase + op3_to_2 + params.op2_feedback * op2_prev;
        let op2_out = (op2_total_phase * TAU).sin() * params.op2_level;
        op2_prev = op2_out;

        let op1_mod = op2_out + op3_to_1;
        let op1_out = ((phase + op1_mod) * TAU).sin();

        if i >= NUM_SAMPLES * WARMUP_CYCLES {
            samples.push(op1_out);
        }
    }
    samples
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

/// Blend a pixel, keeping max coverage so overlapping segments don't wash out.
fn blend_pixel(pixels: &mut [u8], x: u32, y: u32, alpha: f32) {
    if x >= IMG_WIDTH || y >= IMG_HEIGHT {
        return;
    }
    let idx = ((y * IMG_WIDTH + x) * 4) as usize;
    let a = alpha.clamp(0.0, 1.0);
    let existing =
        (pixels[idx + 1] as f32 - BG[1] as f32) / (LINE[1] as f32 - BG[1] as f32).max(1.0);
    let a = a.max(existing.max(0.0));
    let inv = 1.0 - a;
    pixels[idx] = (LINE[0] as f32 * a + BG[0] as f32 * inv) as u8;
    pixels[idx + 1] = (LINE[1] as f32 * a + BG[1] as f32 * inv) as u8;
    pixels[idx + 2] = (LINE[2] as f32 * a + BG[2] as f32 * inv) as u8;
    pixels[idx + 3] = 0xFF;
}

/// Distance from point (px, py) to line segment (x0, y0)-(x1, y1).
fn dist_to_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    let t = (((px - x0) * dx + (py - y0) * dy) / len_sq).clamp(0.0, 1.0);
    let cx = x0 + t * dx;
    let cy = y0 + t * dy;
    ((px - cx) * (px - cx) + (py - cy) * (py - cy)).sqrt()
}

/// Draw a line segment with proper perpendicular distance anti-aliasing.
fn draw_segment(pixels: &mut [u8], x0: f32, y0: f32, x1: f32, y1: f32) {
    let pad = LINE_RADIUS + 1.0;
    let (top, bot) = if y0 < y1 { (y0, y1) } else { (y1, y0) };
    let px_min = (x0.min(x1) - pad).floor().max(0.0) as u32;
    let px_max = (x0.max(x1) + pad).ceil().min(IMG_WIDTH as f32 - 1.0) as u32;
    let py_min = (top - pad).floor().max(0.0) as u32;
    let py_max = (bot + pad).ceil().min(IMG_HEIGHT as f32 - 1.0) as u32;

    for py in py_min..=py_max {
        for px in px_min..=px_max {
            let dist = dist_to_segment(px as f32, py as f32, x0, y0, x1, y1);
            if dist < LINE_RADIUS {
                blend_pixel(pixels, px, py, 1.0);
            } else if dist < LINE_RADIUS + 1.0 {
                blend_pixel(pixels, px, py, 1.0 - (dist - LINE_RADIUS));
            }
        }
    }
}

pub fn render_image(params: &FmWaveformParams) -> Image {
    let samples = compute_waveform(params);
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(IMG_WIDTH, IMG_HEIGHT);
    let pixels = buffer.make_mut_bytes();

    // Fill background
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&BG);
    }

    // Draw center line (2px for retina)
    let mid_y = IMG_HEIGHT / 2;
    for y in mid_y..mid_y + 2 {
        for x in 0..IMG_WIDTH {
            let idx = ((y * IMG_WIDTH + x) * 4) as usize;
            pixels[idx..idx + 4].copy_from_slice(&CENTER_LINE);
        }
    }

    let amplitude = (IMG_HEIGHT as f32 / 2.0) * 0.85;
    let mid = IMG_HEIGHT as f32 / 2.0;

    // Catmull-Rom spline interpolation at pixel column x
    let y_at = |x: u32| -> f32 {
        let t = x as f32 / (IMG_WIDTH - 1) as f32;
        let pos = t * (samples.len() - 1) as f32;
        let i = pos.floor() as i32;
        let frac = pos - i as f32;
        let len = samples.len() as i32;
        let idx = |i: i32| -> usize { i.clamp(0, len - 1) as usize };
        let s = catmull_rom(
            samples[idx(i - 1)],
            samples[idx(i)],
            samples[idx(i + 1)],
            samples[idx(i + 2)],
            frac,
        );
        mid - s * amplitude
    };

    let mut prev_y = y_at(0);
    for x in 1..IMG_WIDTH {
        let cur_y = y_at(x);
        draw_segment(pixels, (x - 1) as f32, prev_y, x as f32, cur_y);
        prev_y = cur_y;
    }

    Image::from_rgba8(buffer)
}
