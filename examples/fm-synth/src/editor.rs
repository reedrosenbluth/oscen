use crate::params::FMParams;
use crate::FMSynth;
use nih_plug::prelude::*;
use nih_plug_slint::create_slint_editor;
use std::sync::Arc;

slint::include_modules!();

/// Set initial UI values from parameter values.
macro_rules! set_initial_values {
    ($ui:expr, $params:expr, [
        $($setter:ident <- $($param_path:ident).+),* $(,)?
    ]) => {
        $(
            $ui.$setter($params.$($param_path).+.value());
        )*
    };
}

/// Bind UI callbacks to parameter setters.
macro_rules! bind_param_callbacks {
    ($ui:expr, $gui_ctx:expr, $params:expr, [
        $($callback:ident => $($param_path:ident).+),* $(,)?
    ]) => {
        $(
            $ui.$callback({
                let gui_context = $gui_ctx.clone();
                let params = $params.clone();
                move |value| {
                    let setter = ParamSetter::new(gui_context.as_ref());
                    setter.begin_set_parameter(&params.$($param_path).+);
                    setter.set_parameter(&params.$($param_path).+, value);
                    setter.end_set_parameter(&params.$($param_path).+);
                }
            });
        )*
    };
}

/// Create an editor for the FM synth plugin.
pub fn create(
    params: Arc<FMParams>,
    _async_executor: AsyncExecutor<FMSynth>,
) -> Option<Box<dyn Editor>> {
    create_slint_editor(params.editor_state.clone(), move |gui_context, mouse_control| {
        let ui = SynthWindow::new().unwrap();

        // Set initial values from params
        set_initial_values!(ui, params, [
            // OP3
            set_op3_ratio <- op3.ratio,
            set_op3_level <- op3.level,
            set_op3_feedback <- op3.feedback,
            set_op3_attack <- op3.attack,
            set_op3_decay <- op3.decay,
            set_op3_sustain <- op3.sustain,
            set_op3_release <- op3.release,
            // OP2
            set_op2_ratio <- op2.ratio,
            set_op2_level <- op2.level,
            set_op2_feedback <- op2.feedback,
            set_op2_attack <- op2.attack,
            set_op2_decay <- op2.decay,
            set_op2_sustain <- op2.sustain,
            set_op2_release <- op2.release,
            // OP1
            set_op1_attack <- op1.attack,
            set_op1_decay <- op1.decay,
            set_op1_sustain <- op1.sustain,
            set_op1_release <- op1.release,
            // Route
            set_route <- route,
            // Filter
            set_filter_cutoff <- filter.cutoff,
            set_filter_resonance <- filter.resonance,
            set_filter_env_amount <- filter.env_amount,
            set_filter_attack <- filter.attack,
            set_filter_decay <- filter.decay,
            set_filter_sustain <- filter.sustain,
            set_filter_release <- filter.release,
        ]);

        // Connect callbacks for parameter editing
        bind_param_callbacks!(ui, gui_context, params, [
            // OP3
            on_op3_ratio_edited => op3.ratio,
            on_op3_level_edited => op3.level,
            on_op3_feedback_edited => op3.feedback,
            on_op3_attack_edited => op3.attack,
            on_op3_decay_edited => op3.decay,
            on_op3_sustain_edited => op3.sustain,
            on_op3_release_edited => op3.release,
            // OP2
            on_op2_ratio_edited => op2.ratio,
            on_op2_level_edited => op2.level,
            on_op2_feedback_edited => op2.feedback,
            on_op2_attack_edited => op2.attack,
            on_op2_decay_edited => op2.decay,
            on_op2_sustain_edited => op2.sustain,
            on_op2_release_edited => op2.release,
            // OP1
            on_op1_attack_edited => op1.attack,
            on_op1_decay_edited => op1.decay,
            on_op1_sustain_edited => op1.sustain,
            on_op1_release_edited => op1.release,
            // Route
            on_route_edited => route,
            // Filter
            on_filter_cutoff_edited => filter.cutoff,
            on_filter_resonance_edited => filter.resonance,
            on_filter_env_amount_edited => filter.env_amount,
            on_filter_attack_edited => filter.attack,
            on_filter_decay_edited => filter.decay,
            on_filter_sustain_edited => filter.sustain,
            on_filter_release_edited => filter.release,
        ]);

        // Drag handlers for unbounded mouse movement
        ui.on_knob_drag_started({
            let mc = mouse_control.clone();
            move || mc.enable_unbounded_movement(true)
        });

        ui.on_knob_drag_ended({
            let mc = mouse_control.clone();
            move || mc.disable_unbounded_movement()
        });

        ui
    })
}
