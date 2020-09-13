use oscen::envelopes;
use oscen::rack::*;
use oscen::utils::signals;
use plotters::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rack = Rack::new();
    let mut controls = Controls::new();
    let mut state = State::new();
    let adsr = envelopes::AdsrBuilder::new()
        .attack(1.0)
        .decay(1.0)
        .release(1.0)
        .sustain(0.8)
        .ax(0.2)
        .dx(0.2)
        .rx(0.2).rack(&mut rack, &mut controls);

    adsr.on(&mut controls, &mut state);
    let mut ad = signals(&mut rack, 0, 4000, 1000.0);
    adsr.off(&mut controls);
    let released = signals(&mut rack, 4001, 5000, 1000.0);
    ad.extend(released);
    let root = SVGBackend::new("adsr.svg", (800, 600)).into_drawing_area();
    root.fill(&BLACK)?;
    let mut chart = ChartBuilder::on(&root)
        .caption("ADSR", ("sans-serif", 50).into_font())
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_ranged(0f32..5f32, 0f32..1.1f32)?;

    chart.configure_mesh().draw()?;

    chart
        .draw_series(LineSeries::new(ad, &YELLOW))?
        .label("Level")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &YELLOW));

    chart
        .configure_series_labels()
        .background_style(&BLACK.mix(0.8))
        .border_style(&BLACK)
        .draw()?;

    Ok(())
}
