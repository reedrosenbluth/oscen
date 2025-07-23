use eframe::egui;
use std::f32::consts::PI;

struct KnobApp {
    volume: f32,
    frequency: f32,
    gain: f32,
}

impl KnobApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        replace_fonts(&cc.egui_ctx);
        customize_colors(&cc.egui_ctx);
        Self {
            volume: 0.5,
            frequency: 440.0,
            gain: 0.0,
        }
    }
}

impl eframe::App for KnobApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("some knobs");
                });
            });
            ui.add_space(30.0);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(100.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("vol").size(16.0));
                        ui.add(knob_with_size(&mut self.volume, 0.0..=1.0, 70.0));
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(format!("{:.2}", self.volume))
                                .size(16.0)
                                .text_style(egui::TextStyle::Monospace),
                        );
                    });
                });

                ui.vertical(|ui| {
                    ui.set_width(100.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("freq").size(16.0));
                        ui.add(knob_with_size(&mut self.frequency, 20.0..=20000.0, 70.0));
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(format!("{:.0}", self.frequency))
                                .size(16.0)
                                .text_style(egui::TextStyle::Monospace),
                        );
                    });
                });

                ui.vertical(|ui| {
                    ui.set_width(100.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("gain").size(16.0));
                        ui.add(knob_with_size(&mut self.gain, -60.0..=12.0, 70.0));
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(format!("{:.1}", self.gain))
                                .size(16.0)
                                .text_style(egui::TextStyle::Monospace),
                        );
                    });
                });
            });
        });
    }
}

// Demonstrates how to replace all fonts.
fn replace_fonts(ctx: &egui::Context) {
    // Start with the default fonts (we will be adding to them rather than replacing them).
    let mut fonts = egui::FontDefinitions::default();

    // Install my own font (maybe supporting non-latin characters).
    // .ttf and .otf files supported.
    fonts.font_data.insert(
        "lars".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "fonts/LarsRoundedTrial-Regular.otf"
        ))),
    );

    // Add another font - replace "your_font_name" and the path with your actual font
    fonts.font_data.insert(
        "mono".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "fonts/LarsMonoTrial-Regular.otf" // or .otf
        ))),
    );

    // Put my font first (highest priority) for proportional text:
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "lars".to_owned());

    // Add the second font as second priority for proportional text:
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(1, "mono".to_owned());

    // Put my font as last fallback for monospace:
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("mono".to_owned());

    // Tell egui to use these fonts:
    ctx.set_fonts(fonts);
}

// Customize the colors of the app
fn customize_colors(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark(); // Start with dark theme

    // Customize background colors
    visuals.panel_fill = egui::Color32::from_rgb(10, 0, 20); // Dark blue-gray background

    // Customize text color
    visuals.override_text_color = Some(egui::Color32::from_rgb(90, 72, 117)); // Light gray text

    // Customize knob colors
    // The knob circle outline color (fg_stroke.color)
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(150, 120, 180); // Purple outline for inactive knobs
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(180, 150, 210); // Lighter purple when hovered
    visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(200, 170, 230); // Even lighter when active

    // Set the custom visuals
    ctx.set_visuals(visuals);
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([200.0, 150.0]) // Set minimum size
            .with_resizable(true), // Make window resizable
        ..Default::default()
    };

    eframe::run_native(
        "Knobs",
        options,
        Box::new(|cc| Ok(Box::new(KnobApp::new(cc)))),
    )
}

pub fn knob_ui(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    // Use the default size based on interact_size and delegate to knob_ui_with_size
    let default_size = ui.spacing().interact_size.y * 2.0;
    knob_ui_with_size(ui, value, range, default_size)
}

/// A wrapper that allows the more idiomatic usage pattern: `ui.add(knob(&mut my_value, 0.0..=1.0))`
///
/// ## Example:
/// ```ignore
/// let mut volume = 0.5;
/// ui.add(knob(&mut volume, 0.0..=1.0));
/// ```
pub fn knob(value: &mut f32, range: std::ops::RangeInclusive<f32>) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| knob_ui(ui, value, range)
}

/// A knob widget with custom size
///
/// ## Example:
/// ```ignore
/// let mut volume = 0.5;
/// ui.add(knob_with_size(&mut volume, 0.0..=1.0, 80.0)); // 80x80 pixels
/// ```
pub fn knob_with_size(
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: f32,
) -> impl egui::Widget + '_ {
    move |ui: &mut egui::Ui| knob_ui_with_size(ui, value, range, size)
}

/// Knob widget implementation with custom size
pub fn knob_ui_with_size(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: f32,
) -> egui::Response {
    // 1. Use the custom size
    let desired_size = egui::vec2(size, size);

    // 2. Allocate space for the knob
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());

    // 3. Handle interactions
    let min_val = *range.start();
    let max_val = *range.end();
    let old_value = *value;

    if response.dragged() {
        // Hide cursor while dragging
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);

        // Use vertical mouse movement for value changes
        let drag_delta = response.drag_delta();
        let sensitivity = 0.01; // Adjust this to make dragging more or less sensitive

        // Dragging up increases value, dragging down decreases value
        let value_change = -drag_delta.y * sensitivity * (max_val - min_val);
        *value = (*value + value_change).clamp(min_val, max_val);

        if *value != old_value {
            response.mark_changed();
        }
    } else if response.drag_stopped() {
        // Restore cursor when drag stops
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }

    // Handle click to jump to position based on vertical position in the knob
    if response.clicked() {
        if let Some(mouse_pos) = response.interact_pointer_pos() {
            let relative_y = (mouse_pos.y - rect.top()) / rect.height();
            // Invert so top = max value, bottom = min value
            let t = 1.0 - relative_y.clamp(0.0, 1.0);
            *value = egui::lerp(min_val..=max_val, t);

            if *value != old_value {
                response.mark_changed();
            }
        }
    }

    // Add widget info for accessibility
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            ui.is_enabled(),
            *value as f64,
            format!("{:.2}", *value).as_str(),
        )
    });

    // 4. Paint the knob (simplified design)
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let center = rect.center();
        let radius = rect.width() * 0.4;

        // Draw just the outer circle outline
        ui.painter().circle_stroke(
            center,
            radius,
            egui::Stroke::new(2.0, visuals.fg_stroke.color),
        );

        // Calculate the notch position
        let normalized_value = (*value - min_val) / (max_val - min_val);

        // Start at bottom-left (225°) and sweep clockwise to bottom-right (315°)
        let notch_angle = egui::lerp(PI * 0.75..=PI * 2.25, normalized_value);

        // Draw the notch indicator (small circle at the edge)
        let notch_start = center
            + egui::vec2(
                (radius * 0.8) * notch_angle.cos(),
                (radius * 0.8) * notch_angle.sin(),
            );

        ui.painter().circle(
            notch_start,
            2.0,
            visuals.text_color(),
            egui::Stroke::new(1.0, visuals.text_color()),
        );
    }

    response
}
