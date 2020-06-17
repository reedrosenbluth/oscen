use plotters::prelude::*;
use swell::envelopes;
use swell::signal::*;
use swell::utils::signals;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut adsr = envelopes::Adsr::linear()
        .attack(1.into())
        .decay(1.into())
        .release(1.into())
        .sustain(0.8.into())
        .build();

    adsr.on();
    let mut ad = signals(&mut adsr, 0, 4000, 1000.0);
    adsr.off();
    let released = signals(&mut adsr, 4001, 5000, 1000.0);
    ad.extend(released);
    let root = SVGBackend::new("adsr.svg", (800, 600)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption("ADSR", ("sans-serif", 50).into_font())
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_ranged(0f32..5f32, 0f32..1.1f32)?;

    chart.configure_mesh().draw()?;

    chart
        .draw_series(LineSeries::new(ad, &BLUE))?
        .label("Level")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &BLUE));

    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.8))
        .border_style(&BLACK)
        .draw()?;

    Ok(())
}
