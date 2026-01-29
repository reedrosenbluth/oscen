use crate::FMSynth;
use crate::FMSynthParams;
use nih_plug::prelude::*;
use nih_plug_slint::create_slint_editor_with_param_callback;
use std::sync::Arc;

slint::include_modules!();

/// Unified UI bindings for NIH-plug parameters.
/// Generates set_ui_values() and bind_ui_callbacks() from a single mapping.
macro_rules! ui_param_bindings {
    (
        ui: $ui_ty:ty;
        params: $params_ty:ty;

        bindings {
            $($ui_name:ident <- $($param_path:ident).+),* $(,)?
        }
    ) => {
        paste::paste! {
            fn set_ui_values(ui: &$ui_ty, params: &$params_ty) {
                $(
                    ui.[<set_ $ui_name>](params.$($param_path).+.value());
                )*
            }

            fn bind_ui_callbacks(
                ui: &$ui_ty,
                gui_ctx: Arc<dyn GuiContext>,
                params: Arc<$params_ty>,
            ) {
                $(
                    ui.[<on_ $ui_name _edited>]({
                        let gui_context = gui_ctx.clone();
                        let params = params.clone();
                        move |value| {
                            let setter = ParamSetter::new(gui_context.as_ref());
                            setter.begin_set_parameter(&params.$($param_path).+);
                            setter.set_parameter(&params.$($param_path).+, value);
                            setter.end_set_parameter(&params.$($param_path).+);
                        }
                    });
                )*
            }
        }
    };
}

ui_param_bindings! {
    ui: SynthWindow;
    params: FMSynthParams;

    bindings {
        // OP3
        op3_ratio <- synth.op3_ratio,
        op3_level <- synth.op3_level,
        op3_feedback <- synth.op3_feedback,
        op3_attack <- synth.op3_attack,
        op3_decay <- synth.op3_decay,
        op3_sustain <- synth.op3_sustain,
        op3_release <- synth.op3_release,
        // OP2
        op2_ratio <- synth.op2_ratio,
        op2_level <- synth.op2_level,
        op2_feedback <- synth.op2_feedback,
        op2_attack <- synth.op2_attack,
        op2_decay <- synth.op2_decay,
        op2_sustain <- synth.op2_sustain,
        op2_release <- synth.op2_release,
        // OP1
        op1_attack <- synth.op1_attack,
        op1_decay <- synth.op1_decay,
        op1_sustain <- synth.op1_sustain,
        op1_release <- synth.op1_release,
        // Route
        route <- synth.route,
        // Filter
        filter_cutoff <- synth.filter_cutoff,
        filter_resonance <- synth.filter_resonance,
        filter_env_amount <- synth.filter_env_amount,
        filter_attack <- synth.filter_attack,
        filter_decay <- synth.filter_decay,
        filter_sustain <- synth.filter_sustain,
        filter_release <- synth.filter_release,
    }
}

/// Create an editor for the FM synth plugin.
pub fn create(
    params: Arc<FMSynthParams>,
    _async_executor: AsyncExecutor<FMSynth>,
) -> Option<Box<dyn Editor>> {
    let params_for_callback = params.clone();

    create_slint_editor_with_param_callback(
        params.editor_state.clone(),
        move |gui_context, mouse_control| {
            let ui = SynthWindow::new().unwrap();

            // Set initial values from params and bind callbacks
            set_ui_values(&ui, &params);
            bind_ui_callbacks(&ui, gui_context.clone(), params.clone());

            // Drag handlers for unbounded mouse movement
            ui.on_knob_drag_started({
                let mc = mouse_control.clone();
                move || mc.enable_unbounded_movement(true)
            });

            ui.on_knob_drag_ended({
                let mc = mouse_control;
                move || mc.disable_unbounded_movement()
            });

            ui
        },
        // Callback invoked when host changes parameters (automation, presets, etc.)
        Some(Arc::new(move |ui: &SynthWindow| {
            set_ui_values(ui, &params_for_callback);
        })),
    )
}
