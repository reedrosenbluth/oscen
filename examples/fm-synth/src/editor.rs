use crate::params::PivotParams;
use crate::Pivot;
use nih_plug::prelude::*;
use nih_plug_slint::create_slint_editor;
use std::sync::Arc;

slint::include_modules!();

/// Create an editor for the Pivot plugin.
pub fn create(
    params: Arc<PivotParams>,
    _async_executor: AsyncExecutor<Pivot>,
) -> Option<Box<dyn Editor>> {
    create_slint_editor(params.editor_state.clone(), move |gui_context, mouse_control| {
        let ui = SynthWindow::new().unwrap();

        // Set initial values from params
        // OP3
        ui.set_op3_ratio(params.op3.ratio.value());
        ui.set_op3_level(params.op3.level.value());
        ui.set_op3_feedback(params.op3.feedback.value());
        ui.set_op3_attack(params.op3.attack.value());
        ui.set_op3_decay(params.op3.decay.value());
        ui.set_op3_sustain(params.op3.sustain.value());
        ui.set_op3_release(params.op3.release.value());

        // OP2
        ui.set_op2_ratio(params.op2.ratio.value());
        ui.set_op2_level(params.op2.level.value());
        ui.set_op2_feedback(params.op2.feedback.value());
        ui.set_op2_attack(params.op2.attack.value());
        ui.set_op2_decay(params.op2.decay.value());
        ui.set_op2_sustain(params.op2.sustain.value());
        ui.set_op2_release(params.op2.release.value());

        // OP1
        ui.set_op1_attack(params.op1.attack.value());
        ui.set_op1_decay(params.op1.decay.value());
        ui.set_op1_sustain(params.op1.sustain.value());
        ui.set_op1_release(params.op1.release.value());

        // Route
        ui.set_route(params.route.value());

        // Filter
        ui.set_filter_cutoff(params.filter.cutoff.value());
        ui.set_filter_resonance(params.filter.resonance.value());
        ui.set_filter_env_amount(params.filter.env_amount.value());
        ui.set_filter_attack(params.filter.attack.value());
        ui.set_filter_decay(params.filter.decay.value());
        ui.set_filter_sustain(params.filter.sustain.value());
        ui.set_filter_release(params.filter.release.value());

        // Connect callbacks for parameter editing
        // OP3 callbacks
        ui.on_op3_ratio_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.ratio);
                setter.set_parameter(&params.op3.ratio, value);
                setter.end_set_parameter(&params.op3.ratio);
            }
        });

        ui.on_op3_level_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.level);
                setter.set_parameter(&params.op3.level, value);
                setter.end_set_parameter(&params.op3.level);
            }
        });

        ui.on_op3_feedback_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.feedback);
                setter.set_parameter(&params.op3.feedback, value);
                setter.end_set_parameter(&params.op3.feedback);
            }
        });

        ui.on_op3_attack_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.attack);
                setter.set_parameter(&params.op3.attack, value);
                setter.end_set_parameter(&params.op3.attack);
            }
        });

        ui.on_op3_decay_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.decay);
                setter.set_parameter(&params.op3.decay, value);
                setter.end_set_parameter(&params.op3.decay);
            }
        });

        ui.on_op3_sustain_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.sustain);
                setter.set_parameter(&params.op3.sustain, value);
                setter.end_set_parameter(&params.op3.sustain);
            }
        });

        ui.on_op3_release_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op3.release);
                setter.set_parameter(&params.op3.release, value);
                setter.end_set_parameter(&params.op3.release);
            }
        });

        // OP2 callbacks
        ui.on_op2_ratio_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.ratio);
                setter.set_parameter(&params.op2.ratio, value);
                setter.end_set_parameter(&params.op2.ratio);
            }
        });

        ui.on_op2_level_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.level);
                setter.set_parameter(&params.op2.level, value);
                setter.end_set_parameter(&params.op2.level);
            }
        });

        ui.on_op2_feedback_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.feedback);
                setter.set_parameter(&params.op2.feedback, value);
                setter.end_set_parameter(&params.op2.feedback);
            }
        });

        ui.on_op2_attack_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.attack);
                setter.set_parameter(&params.op2.attack, value);
                setter.end_set_parameter(&params.op2.attack);
            }
        });

        ui.on_op2_decay_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.decay);
                setter.set_parameter(&params.op2.decay, value);
                setter.end_set_parameter(&params.op2.decay);
            }
        });

        ui.on_op2_sustain_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.sustain);
                setter.set_parameter(&params.op2.sustain, value);
                setter.end_set_parameter(&params.op2.sustain);
            }
        });

        ui.on_op2_release_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op2.release);
                setter.set_parameter(&params.op2.release, value);
                setter.end_set_parameter(&params.op2.release);
            }
        });

        // OP1 callbacks
        ui.on_op1_attack_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op1.attack);
                setter.set_parameter(&params.op1.attack, value);
                setter.end_set_parameter(&params.op1.attack);
            }
        });

        ui.on_op1_decay_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op1.decay);
                setter.set_parameter(&params.op1.decay, value);
                setter.end_set_parameter(&params.op1.decay);
            }
        });

        ui.on_op1_sustain_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op1.sustain);
                setter.set_parameter(&params.op1.sustain, value);
                setter.end_set_parameter(&params.op1.sustain);
            }
        });

        ui.on_op1_release_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.op1.release);
                setter.set_parameter(&params.op1.release, value);
                setter.end_set_parameter(&params.op1.release);
            }
        });

        // Route callback
        ui.on_route_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.route);
                setter.set_parameter(&params.route, value);
                setter.end_set_parameter(&params.route);
            }
        });

        // Filter callbacks
        ui.on_filter_cutoff_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.cutoff);
                setter.set_parameter(&params.filter.cutoff, value);
                setter.end_set_parameter(&params.filter.cutoff);
            }
        });

        ui.on_filter_resonance_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.resonance);
                setter.set_parameter(&params.filter.resonance, value);
                setter.end_set_parameter(&params.filter.resonance);
            }
        });

        ui.on_filter_env_amount_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.env_amount);
                setter.set_parameter(&params.filter.env_amount, value);
                setter.end_set_parameter(&params.filter.env_amount);
            }
        });

        ui.on_filter_attack_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.attack);
                setter.set_parameter(&params.filter.attack, value);
                setter.end_set_parameter(&params.filter.attack);
            }
        });

        ui.on_filter_decay_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.decay);
                setter.set_parameter(&params.filter.decay, value);
                setter.end_set_parameter(&params.filter.decay);
            }
        });

        ui.on_filter_sustain_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.sustain);
                setter.set_parameter(&params.filter.sustain, value);
                setter.end_set_parameter(&params.filter.sustain);
            }
        });

        ui.on_filter_release_edited({
            let gui_context = gui_context.clone();
            let params = params.clone();
            move |value| {
                let setter = ParamSetter::new(gui_context.as_ref());
                setter.begin_set_parameter(&params.filter.release);
                setter.set_parameter(&params.filter.release, value);
                setter.end_set_parameter(&params.filter.release);
            }
        });

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
