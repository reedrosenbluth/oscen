use eframe::egui;
use std::f32::consts::PI;

/// Specification for knob behavior (similar to egui's SliderSpec)
#[derive(Clone)]
struct KnobSpec {
    logarithmic: bool,
    /// For logarithmic knobs, the smallest positive value we are interested in.
    /// 1 for integer knobs, maybe 1e-6 for others.
    smallest_positive: f64,
    /// For logarithmic knobs, the largest positive value we are interested in
    /// before the knob switches to `INFINITY`, if that is the higher end.
    /// Default: INFINITY.
    largest_finite: f64,
}

impl Default for KnobSpec {
    fn default() -> Self {
        Self {
            logarithmic: false,
            smallest_positive: 1e-6,
            largest_finite: f64::INFINITY,
        }
    }
}

/// A knob widget for egui applications, similar to egui::Slider
///
/// ## Example:
/// ```ignore
/// let mut volume = 0.5;
/// ui.add(Knob::new(&mut volume, 0.0..=1.0));
///
/// let mut frequency = 440.0;
/// ui.add(Knob::new(&mut frequency, 20.0..=20000.0).logarithmic(true));
/// ```
pub struct Knob<'a> {
    value: &'a mut f32,
    range: std::ops::RangeInclusive<f32>,
    spec: KnobSpec,
    size: Option<f32>,
}

impl<'a> Knob<'a> {
    pub fn new(value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        Self {
            value,
            range,
            spec: KnobSpec::default(),
            size: None,
        }
    }

    /// Make this knob logarithmic (default: false).
    /// This is useful for frequency parameters and other values that span multiple orders of magnitude.
    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        self.spec.logarithmic = logarithmic;
        self
    }

    /// For logarithmic knobs, the smallest positive value we are interested in.
    /// 1 for integer knobs, maybe 1e-6 for others.
    pub fn smallest_positive(mut self, smallest_positive: f64) -> Self {
        self.spec.smallest_positive = smallest_positive;
        self
    }

    /// For logarithmic knobs, the largest positive value we are interested in
    /// before the knob switches to `INFINITY`, if that is the higher end.
    /// Default: INFINITY.
    pub fn largest_finite(mut self, largest_finite: f64) -> Self {
        self.spec.largest_finite = largest_finite;
        self
    }

    /// Set a custom size for the knob (default: based on ui.spacing().interact_size)
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }
}

impl<'a> egui::Widget for Knob<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let size = self
            .size
            .unwrap_or_else(|| ui.spacing().interact_size.y * 2.0);
        knob_ui_with_options(ui, self.value, self.range, size, self.spec)
    }
}

/// Legacy function for backward compatibility
/// ## Example:
/// ```ignore
/// let mut volume = 0.5;
/// ui.add(knob(&mut volume, 0.0..=1.0));
/// ```
pub fn knob(value: &mut f32, range: std::ops::RangeInclusive<f32>) -> Knob {
    Knob::new(value, range)
}

/// Legacy function for backward compatibility
pub fn knob_ui(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> egui::Response {
    ui.add(Knob::new(value, range))
}

/// Legacy function for backward compatibility
pub fn knob_with_size(value: &mut f32, range: std::ops::RangeInclusive<f32>, size: f32) -> Knob {
    Knob::new(value, range).size(size)
}

/// Legacy function for backward compatibility
pub fn knob_logarithmic(value: &mut f32, range: std::ops::RangeInclusive<f32>) -> Knob {
    Knob::new(value, range).logarithmic(true)
}

/// Legacy function for backward compatibility
pub fn knob_logarithmic_with_size(
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: f32,
) -> Knob {
    Knob::new(value, range).logarithmic(true).size(size)
}

/// Legacy function for backward compatibility
pub fn knob_ui_with_size(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: f32,
) -> egui::Response {
    ui.add(Knob::new(value, range).size(size))
}

/// Core knob widget implementation with all options
pub fn knob_ui_with_options(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    size: f32,
    spec: KnobSpec,
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
        let normalized_change = -drag_delta.y * sensitivity;

        // Convert current value to normalized [0, 1] range
        let current_normalized =
            normalized_from_value(*value as f64, min_val as f64..=max_val as f64, &spec);

        // Apply the change in normalized space
        let new_normalized = (current_normalized + normalized_change as f64).clamp(0.0, 1.0);

        // Convert back to value space
        let new_value =
            value_from_normalized(new_normalized, min_val as f64..=max_val as f64, &spec) as f32;

        *value = new_value.clamp(min_val, max_val);

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
            let normalized_pos = (1.0 - relative_y.clamp(0.0, 1.0)) as f64;

            // Convert from normalized position to actual value
            let new_value =
                value_from_normalized(normalized_pos, min_val as f64..=max_val as f64, &spec)
                    as f32;
            *value = new_value.clamp(min_val, max_val);

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

        // Calculate the notch position using normalized value
        let normalized_value =
            normalized_from_value(*value as f64, min_val as f64..=max_val as f64, &spec);

        // Start at bottom-left (225°) and sweep clockwise to bottom-right (315°)
        let notch_angle = egui::lerp(PI * 0.75..=PI * 2.25, normalized_value as f32);

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

// Helper functions for logarithmic scaling (adapted from egui's slider implementation)

const INFINITY: f64 = f64::INFINITY;

/// When the user asks for an infinitely large range (e.g. logarithmic from zero),
/// give a scale that this many orders of magnitude in size.
const INF_RANGE_MAGNITUDE: f64 = 10.0;

fn value_from_normalized(
    normalized: f64,
    range: std::ops::RangeInclusive<f64>,
    spec: &KnobSpec,
) -> f64 {
    let (min, max) = (*range.start(), *range.end());

    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        min
    } else if min > max {
        value_from_normalized(1.0 - normalized, max..=min, spec)
    } else if normalized <= 0.0 {
        min
    } else if normalized >= 1.0 {
        max
    } else if spec.logarithmic {
        if max <= 0.0 {
            // non-positive range
            -value_from_normalized(normalized, -min..=-max, spec)
        } else if 0.0 <= min {
            let (min_log, max_log) = range_log10(min, max, spec);
            let log = egui::lerp(min_log..=max_log, normalized);
            10.0_f64.powf(log)
        } else {
            assert!(min < 0.0 && 0.0 < max);
            let zero_cutoff = logarithmic_zero_cutoff(min, max);
            if normalized < zero_cutoff {
                // negative
                value_from_normalized(
                    egui::remap(normalized, 0.0..=zero_cutoff, 0.0..=1.0),
                    min..=0.0,
                    spec,
                )
            } else {
                // positive
                value_from_normalized(
                    egui::remap(normalized, zero_cutoff..=1.0, 0.0..=1.0),
                    0.0..=max,
                    spec,
                )
            }
        }
    } else {
        debug_assert!(
            min.is_finite() && max.is_finite(),
            "You should use a logarithmic range"
        );
        egui::lerp(range, normalized.clamp(0.0, 1.0))
    }
}

fn normalized_from_value(value: f64, range: std::ops::RangeInclusive<f64>, spec: &KnobSpec) -> f64 {
    let (min, max) = (*range.start(), *range.end());

    if min.is_nan() || max.is_nan() {
        f64::NAN
    } else if min == max {
        0.5 // empty range, show center of knob
    } else if min > max {
        1.0 - normalized_from_value(value, max..=min, spec)
    } else if value <= min {
        0.0
    } else if value >= max {
        1.0
    } else if spec.logarithmic {
        if max <= 0.0 {
            // non-positive range
            normalized_from_value(-value, -min..=-max, spec)
        } else if 0.0 <= min {
            let (min_log, max_log) = range_log10(min, max, spec);
            let value_log = value.log10();
            egui::remap_clamp(value_log, min_log..=max_log, 0.0..=1.0)
        } else {
            assert!(min < 0.0 && 0.0 < max);
            let zero_cutoff = logarithmic_zero_cutoff(min, max);
            if value < 0.0 {
                // negative
                egui::remap(
                    normalized_from_value(value, min..=0.0, spec),
                    0.0..=1.0,
                    0.0..=zero_cutoff,
                )
            } else {
                // positive side
                egui::remap(
                    normalized_from_value(value, 0.0..=max, spec),
                    0.0..=1.0,
                    zero_cutoff..=1.0,
                )
            }
        }
    } else {
        debug_assert!(
            min.is_finite() && max.is_finite(),
            "You should use a logarithmic range"
        );
        egui::remap_clamp(value, range, 0.0..=1.0)
    }
}

fn range_log10(min: f64, max: f64, spec: &KnobSpec) -> (f64, f64) {
    assert!(spec.logarithmic);
    assert!(min <= max);

    if min == 0.0 && max == INFINITY {
        (spec.smallest_positive.log10(), INF_RANGE_MAGNITUDE)
    } else if min == 0.0 {
        if spec.smallest_positive < max {
            (spec.smallest_positive.log10(), max.log10())
        } else {
            (max.log10() - INF_RANGE_MAGNITUDE, max.log10())
        }
    } else if max == INFINITY {
        if min < spec.largest_finite {
            (min.log10(), spec.largest_finite.log10())
        } else {
            (min.log10(), min.log10() + INF_RANGE_MAGNITUDE)
        }
    } else {
        (min.log10(), max.log10())
    }
}

/// where to put the zero cutoff for logarithmic knobs
/// that crosses zero ?
fn logarithmic_zero_cutoff(min: f64, max: f64) -> f64 {
    assert!(min < 0.0 && 0.0 < max);

    let min_magnitude = if min == -INFINITY {
        INF_RANGE_MAGNITUDE
    } else {
        min.abs().log10().abs()
    };
    let max_magnitude = if max == INFINITY {
        INF_RANGE_MAGNITUDE
    } else {
        max.log10().abs()
    };

    let cutoff = min_magnitude / (min_magnitude + max_magnitude);
    debug_assert!(
        0.0 <= cutoff && cutoff <= 1.0,
        "Bad cutoff {cutoff:?} for min {min:?} and max {max:?}"
    );
    cutoff
}
