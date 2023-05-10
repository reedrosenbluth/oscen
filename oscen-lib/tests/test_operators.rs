use oscen::operators::*;
use oscen::oscillators::*;
use oscen::rack::*;

#[test]
fn mixer() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    let c2 = KonstBuilder::new(2.0.into()).rack(&mut rack, &mut controls);
    let c3 = KonstBuilder::new(3.0.into()).rack(&mut rack, &mut controls);
    MixerBuilder::new(vec![c2.tag(), c3.tag(), c2.tag()]).rack(&mut rack, &mut controls);
    let r1 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r2 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!((r1, r2), (7.0, 7.0));
}

#[test]
fn prod() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    let c2 = KonstBuilder::new(2.0.into()).rack(&mut rack, &mut controls);
    let c3 = KonstBuilder::new(3.0.into()).rack(&mut rack, &mut controls);
    ProductBuilder::new(vec![c2.tag(), c3.tag(), c2.tag()]).rack(&mut rack, &mut controls);
    let r1 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    let r2 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!((r1, r2), (12.0, 12.0));
}

#[test]
fn union() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    let c2 = KonstBuilder::new(2.0.into()).rack(&mut rack, &mut controls);
    let c3 = KonstBuilder::new(3.0.into()).rack(&mut rack, &mut controls);
    let c4 = KonstBuilder::new(4.0.into()).rack(&mut rack, &mut controls);
    let u = UnionBuilder::new(vec![c2.tag(), c3.tag(), c4.tag()]).rack(&mut rack, &mut controls);
    let r1 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    u.set_active(&mut controls, 1.into());
    let r2 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    u.set_active(&mut controls, 2.into());
    let r3 = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!((r1, r2, r3), (2.0, 3.0, 4.0));
}

#[test]
fn vca() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    let c2 = KonstBuilder::new(2.0.into()).rack(&mut rack, &mut controls);
    let vca = VcaBuilder::new(c2.tag()).rack(&mut rack, &mut controls);
    vca.set_level(&mut controls, 2.5.into());
    let r = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!(r, 5.0);
}

#[test]
fn cross() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    let c2 = KonstBuilder::new(2.0.into()).rack(&mut rack, &mut controls);
    let c3 = KonstBuilder::new(3.0.into()).rack(&mut rack, &mut controls);
    let cf = CrossFadeBuilder::new(c2.tag(), c3.tag()).rack(&mut rack, &mut controls);
    cf.set_alpha(&mut controls, 0.25.into());
    let r = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!(r, 2.25);
}

#[test]
fn modulator() {
    let (mut rack, mut controls, mut state, mut outputs, mut buffers) = tables();
    ModulatorBuilder::new(|_, _| 2.0)
        .hz(220.0)
        .ratio(2.0)
        .index(4.0)
        .rack(&mut rack, &mut controls, &mut state);
    let r = rack.mono(&controls, &mut state, &mut outputs, &mut buffers, 1f32);
    assert_eq!(r, 3740.0);
}
