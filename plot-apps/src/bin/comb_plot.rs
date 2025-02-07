use oscen::filters::CombBuilder;
use oscen::oscillators::*;
use oscen::rack::*;
use oscen::utils::signals;
use plotters::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rack = Rack::new();
    let mut controls = Controls::new();
    let mut buffers = Buffers::new();
    let sine = OscBuilder::new(sine_osc)
        .hz(440.0)
        .rack(&mut rack, &mut controls, &mut state);
    CombBuilder::new(sine.tag(), 501).rack(&mut rack, &mut controls, &mut buffers);
    let sigs = signals(
        &mut rack,
        &mut controls,
        &mut state,
        &mut buffers,
        0,
        4000,
        44100.0,
    );
    let root = SVGBackend::new("comb.svg", (800, 600)).into_drawing_area();
    root.fill(&BLACK)?;
    let mut chart = ChartBuilder::on(&root)
        .caption("COMB", ("sans-serif", 50).into_font())
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_ranged(0f32..0.1f32, 0f32..1.1f32)?;

    chart.configure_mesh().draw()?;

    chart
        .draw_series(LineSeries::new(sigs, &GREEN))?
        .label("Level")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLUE));

    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.5))
        .border_style(&BLACK)
        .draw()?;

    Ok(())
}
