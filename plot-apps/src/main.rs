use plotters::prelude::*;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // let root = BitMapBackend::new("plotters-doc-data/0.png", (640, 480)).into_drawing_area();
    let root = SVGBackend::new("adsr.svg", (800, 600)).into_drawing_area();
    root.fill(&WHITE)?;
    let mut chart = ChartBuilder::on(&root)
        .caption("ADSR", ("sans-serif", 50).into_font())
        .margin(5)
        .x_label_area_size(30)
        .y_label_area_size(30)
        .build_ranged(-6f32..6f32, -1.1f32..1.1f32)?;

    chart.configure_mesh().draw()?;

    chart
        .draw_series(LineSeries::new(
            (-600..=600).map(|x| x as f32 / 100.0).map(|x| (x, x.sin())),
            &BLUE,
        ))?
        .label("y = sin x")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], &RED));

    chart
        .configure_series_labels()
        .background_style(&WHITE.mix(0.8))
        .border_style(&BLACK)
        .draw()?;

    Ok(())
}