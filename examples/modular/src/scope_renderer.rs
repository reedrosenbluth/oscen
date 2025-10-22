//! Oscilloscope waveform rendering module
//!
//! This module handles rendering audio waveforms to pixel buffers for display.
//! It uses Xiaolin Wu's line algorithm for anti-aliased rendering.

use oscen::OscilloscopeHandle;
use slint::{Rgb8Pixel, SharedPixelBuffer};

// Rendering constants
const SUPERSAMPLES_PER_COLUMN: usize = 6;
const BACKGROUND: [u8; 3] = [27, 36, 32];
const AXIS_COLOR: [u8; 3] = [60, 72, 68];
const WAVE_COLOR: [u8; 3] = [138, 198, 255];

/// Renders an oscilloscope waveform to a pixel buffer
pub fn render_waveform(
    handle: &OscilloscopeHandle,
    width: u32,
    height: u32,
) -> SharedPixelBuffer<Rgb8Pixel> {
    let width = width.max(1);
    let height = height.max(1);
    let width_usize = width as usize;
    let height_usize = height as usize;
    let snapshot_len = width_usize
        .saturating_mul(SUPERSAMPLES_PER_COLUMN.max(1))
        .max(width_usize * 4);

    let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
    let snapshot = handle.snapshot(snapshot_len.max(width_usize));

    {
        let pixels = buffer.make_mut_slice();
        fill_background(pixels, BACKGROUND);
        draw_axis(pixels, width_usize, height_usize, AXIS_COLOR);
        let samples = if !snapshot.triggered().is_empty() {
            snapshot.triggered()
        } else {
            snapshot.samples()
        };
        draw_waveform(pixels, width_usize, height_usize, samples, WAVE_COLOR);
    }

    buffer
}

fn fill_background(pixels: &mut [Rgb8Pixel], color: [u8; 3]) {
    for px in pixels.iter_mut() {
        *px = Rgb8Pixel::new(color[0], color[1], color[2]);
    }
}

fn draw_axis(pixels: &mut [Rgb8Pixel], width: usize, height: usize, color: [u8; 3]) {
    if height == 0 {
        return;
    }
    let y = height / 2;
    for x in 0..width {
        let idx = y * width + x;
        pixels[idx] = Rgb8Pixel::new(color[0], color[1], color[2]);
    }
}

fn draw_waveform(
    pixels: &mut [Rgb8Pixel],
    width: usize,
    height: usize,
    samples: &[f32],
    color: [u8; 3],
) {
    if width == 0 || height == 0 || samples.is_empty() {
        return;
    }

    let center = (height as f32 - 1.0) / 2.0;
    let scale = center * 0.85;
    let supersamples = SUPERSAMPLES_PER_COLUMN.max(1);
    let first_sample = average_column_sample(samples, 0, width, supersamples);
    let mut prev_x = 0.0f32;
    let mut prev_y = sample_to_y(first_sample, center, scale, height);
    draw_line_segment(pixels, width, height, prev_x, prev_y, prev_x, prev_y, color);

    for x in 1..width {
        let sample = average_column_sample(samples, x, width, supersamples);
        let current_y = sample_to_y(sample, center, scale, height);
        let current_x = x as f32;
        draw_line_segment(
            pixels, width, height, prev_x, prev_y, current_x, current_y, color,
        );
        prev_x = current_x;
        prev_y = current_y;
    }
}

fn sample_at(samples: &[f32], t: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let max_index = (samples.len() - 1) as f32;
    let position = t * max_index;
    let idx0 = position.floor() as usize;
    let idx1 = position.ceil().min((samples.len() - 1) as f32) as usize;
    let frac = position - idx0 as f32;
    let s0 = samples[idx0];
    let s1 = samples[idx1];
    s0 + (s1 - s0) * frac
}

fn sample_to_y(sample: f32, center: f32, scale: f32, height: usize) -> f32 {
    let clamped = sample.clamp(-1.0, 1.0);
    let y = center - clamped * scale;
    y.clamp(0.0, height as f32 - 1.0)
}

fn average_column_sample(samples: &[f32], column: usize, width: usize, supersamples: usize) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    if width <= 1 || supersamples <= 1 {
        let t = if width > 1 {
            column as f32 / (width - 1) as f32
        } else {
            0.0
        };
        return sample_at(samples, t);
    }

    let max_column = (width - 1) as f32;
    let denom = max_column.max(1.0);
    let mut accum = 0.0;
    let supersamples_f = supersamples as f32;
    for i in 0..supersamples {
        let offset = (i as f32 + 0.5) / supersamples_f - 0.5;
        let sample_pos = (column as f32 + offset).clamp(0.0, max_column);
        let t = sample_pos / denom;
        accum += sample_at(samples, t);
    }
    accum / supersamples_f
}

/// Xiaolin Wu's line algorithm for anti-aliased line drawing
fn draw_line_segment(
    pixels: &mut [Rgb8Pixel],
    width: usize,
    height: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: [u8; 3],
) {
    if width == 0 || height == 0 {
        return;
    }

    if (x0 - x1).abs() < f32::EPSILON && (y0 - y1).abs() < f32::EPSILON {
        blend_pixel(
            pixels,
            width,
            height,
            x0.round() as i32,
            y0.round() as i32,
            color,
            1.0,
        );
        return;
    }

    let mut x0 = x0;
    let mut y0 = y0;
    let mut x1 = x1;
    let mut y1 = y1;

    let steep = (y1 - y0).abs() > (x1 - x0).abs();
    if steep {
        std::mem::swap(&mut x0, &mut y0);
        std::mem::swap(&mut x1, &mut y1);
    }
    if x0 > x1 {
        std::mem::swap(&mut x0, &mut x1);
        std::mem::swap(&mut y0, &mut y1);
    }

    let dx = x1 - x0;
    let dy = y1 - y0;
    let gradient = if dx.abs() < f32::EPSILON {
        0.0
    } else {
        dy / dx
    };

    // handle first endpoint
    let x_end = x0.round();
    let y_end = y0 + gradient * (x_end - x0);
    let x_gap = rfpart(x0 + 0.5);
    let xpxl1 = x_end as i32;
    let ypxl1 = ipart(y_end);
    if steep {
        blend_pixel(
            pixels,
            width,
            height,
            ypxl1,
            xpxl1,
            color,
            rfpart(y_end) * x_gap,
        );
        blend_pixel(
            pixels,
            width,
            height,
            ypxl1 + 1,
            xpxl1,
            color,
            fpart(y_end) * x_gap,
        );
    } else {
        blend_pixel(
            pixels,
            width,
            height,
            xpxl1,
            ypxl1,
            color,
            rfpart(y_end) * x_gap,
        );
        blend_pixel(
            pixels,
            width,
            height,
            xpxl1,
            ypxl1 + 1,
            color,
            fpart(y_end) * x_gap,
        );
    }
    let mut intery = y_end + gradient;

    // handle second endpoint
    let x_end = x1.round();
    let y_end = y1 + gradient * (x_end - x1);
    let x_gap = fpart(x1 + 0.5);
    let xpxl2 = x_end as i32;
    let ypxl2 = ipart(y_end);
    if steep {
        blend_pixel(
            pixels,
            width,
            height,
            ypxl2,
            xpxl2,
            color,
            rfpart(y_end) * x_gap,
        );
        blend_pixel(
            pixels,
            width,
            height,
            ypxl2 + 1,
            xpxl2,
            color,
            fpart(y_end) * x_gap,
        );
    } else {
        blend_pixel(
            pixels,
            width,
            height,
            xpxl2,
            ypxl2,
            color,
            rfpart(y_end) * x_gap,
        );
        blend_pixel(
            pixels,
            width,
            height,
            xpxl2,
            ypxl2 + 1,
            color,
            fpart(y_end) * x_gap,
        );
    }

    // main loop
    if xpxl1 + 1 >= xpxl2 {
        return;
    }

    for x in (xpxl1 + 1)..xpxl2 {
        if steep {
            let y = ipart(intery);
            blend_pixel(pixels, width, height, y, x, color, rfpart(intery));
            blend_pixel(pixels, width, height, y + 1, x, color, fpart(intery));
        } else {
            let y = ipart(intery);
            blend_pixel(pixels, width, height, x, y, color, rfpart(intery));
            blend_pixel(pixels, width, height, x, y + 1, color, fpart(intery));
        }
        intery += gradient;
    }
}

fn blend_pixel(
    pixels: &mut [Rgb8Pixel],
    width: usize,
    height: usize,
    x: i32,
    y: i32,
    color: [u8; 3],
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }

    let idx = (y as usize) * width + (x as usize);
    let existing = pixels[idx];
    let alpha = alpha.clamp(0.0, 1.0);
    let inv_alpha = 1.0 - alpha;

    let blended_r = (existing.r as f32 * inv_alpha + color[0] as f32 * alpha)
        .round()
        .clamp(0.0, 255.0) as u8;
    let blended_g = (existing.g as f32 * inv_alpha + color[1] as f32 * alpha)
        .round()
        .clamp(0.0, 255.0) as u8;
    let blended_b = (existing.b as f32 * inv_alpha + color[2] as f32 * alpha)
        .round()
        .clamp(0.0, 255.0) as u8;

    pixels[idx] = Rgb8Pixel::new(blended_r, blended_g, blended_b);
}

fn ipart(x: f32) -> i32 {
    x.floor() as i32
}

fn fpart(x: f32) -> f32 {
    let frac = x - x.floor();
    if frac < 0.0 {
        frac + 1.0
    } else {
        frac
    }
}

fn rfpart(x: f32) -> f32 {
    1.0 - fpart(x)
}
