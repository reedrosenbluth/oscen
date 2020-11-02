use nannou::prelude::*;

pub fn scope_data(data: &[f32]) -> Vec<f32> {
    // Draw Oscilloscope
    let mut scope_data = data.iter().peekable();
    let mut shifted_scope_data: Vec<f32> = vec![];

    for (i, amp) in scope_data.clone().enumerate() {
        if *amp <= 0.0 && scope_data.peek().unwrap_or(&amp) > &&0.0 {
            shifted_scope_data = data[i..].to_vec();
            break;
        }
    }
    shifted_scope_data
}

pub fn scope(app: &App, data: &[f32], frame: Frame) {
    // Draw BG
    let draw = app.draw();
    let bg_color = rgb(9. / 255., 9. / 255., 44. / 255.);
    draw.background().color(bg_color);
    if frame.nth() == 0 {
        draw.to_frame(app, &frame).unwrap()
    }

    // Draw Oscilloscope
    let shifted_scope_data = scope_data(data);

    if shifted_scope_data.len() >= 600 {
        let shifted_scope_data = shifted_scope_data[0..600].iter();
        let scope_points = shifted_scope_data
            .zip((0..600).into_iter())
            .map(|(y, x)| pt2(x as f32, y * 120.));

        draw.path()
            .stroke()
            .weight(2.)
            .points(scope_points)
            .color(CORNFLOWERBLUE)
            .x_y(-295., 0.);

        draw.to_frame(app, &frame).unwrap();
    }
}
