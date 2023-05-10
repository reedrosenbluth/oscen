use oscen::oscillators::*;
use oscen::rack::*;

#[test]
fn osc() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    let o = OscBuilder::new(|x, y| x + y).rack(&mut rack, &mut controls, &mut state);
    o.set_hz(&mut controls, 0.5.into());
    o.set_arg(&mut controls, 7.0.into());
    let r1 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r2 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r3 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!((r1, r2, r3), (7.0, 7.5, 7.0));
}

#[test]
fn cnst() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    KonstBuilder::new(42.0.into()).rack(&mut rack, &mut controls);
    let r = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!(r, 42.0);
}

#[test]
fn clock() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    ClockBuilder::new(3.0).rack(&mut rack, &mut controls);
    let r1 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r2 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r3 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r4 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!((r1, r2, r3, r4), (1.0, 0.0, 0.0, 1.0));
}
