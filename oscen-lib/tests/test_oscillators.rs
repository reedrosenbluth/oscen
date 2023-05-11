use oscen::oscillators::*;
use oscen::rack::*;

#[test]
fn osc() {
    let mut rack = Rack::default();
    let o = OscBuilder::new(|x, y| x + y).rack(&mut rack);
    o.set_hz(&mut rack, 0.5.into());
    o.set_arg(&mut rack, 7.0.into());
    let r1 = rack.mono(1f32);
    let r2 = rack.mono(1f32);
    let r3 = rack.mono(1f32);
    assert_eq!((r1, r2, r3), (7.0, 7.5, 7.0));
}

#[test]
fn cnst() {
    let mut rack = Rack::default();
    ConstBuilder::new(42.0.into()).rack(&mut rack);
    let r = rack.mono(1f32);
    assert_eq!(r, 42.0);
}

#[test]
fn clock() {
    let mut rack = Rack::default();
    ClockBuilder::new(3.0).rack(&mut rack);
    let r1 = rack.mono(1f32);
    let r2 = rack.mono(1f32);
    let r3 = rack.mono(1f32);
    let r4 = rack.mono(1f32);
    assert_eq!((r1, r2, r3, r4), (1.0, 0.0, 0.0, 1.0));
}
