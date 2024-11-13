use oscen::operators::*;
use oscen::oscillators::*;
use oscen::rack::*;

#[test]
fn mixer() {
    let mut rack = Rack::default();
    let c2 = ConstBuilder::new(2.0.into()).rack(&mut rack);
    let c3 = ConstBuilder::new(3.0.into()).rack(&mut rack);
    MixerBuilder::new(vec![
        c2.lock().unwrap().tag(),
        c3.lock().unwrap().tag(),
        c2.lock().unwrap().tag(),
    ])
    .rack(&mut rack);
    let r1 = rack.mono(1f32);
    let r2 = rack.mono(1f32);
    assert_eq!((r1, r2), (7.0, 7.0));
}

#[test]
fn prod() {
    let mut rack = Rack::default();
    let c2 = ConstBuilder::new(2.0.into()).rack(&mut rack);
    let c3 = ConstBuilder::new(3.0.into()).rack(&mut rack);
    ProductBuilder::new(vec![
        c2.lock().unwrap().tag(),
        c3.lock().unwrap().tag(),
        c2.lock().unwrap().tag(),
    ])
    .rack(&mut rack);
    let r1 = rack.mono(1f32);
    let r2 = rack.mono(1f32);
    assert_eq!((r1, r2), (12.0, 12.0));
}

#[test]
fn union() {
    let mut rack = Rack::default();
    let c2 = ConstBuilder::new(2.0.into()).rack(&mut rack);
    let c3 = ConstBuilder::new(3.0.into()).rack(&mut rack);
    let c4 = ConstBuilder::new(4.0.into()).rack(&mut rack);
    let u = UnionBuilder::new(vec![
        c2.lock().unwrap().tag(),
        c3.lock().unwrap().tag(),
        c4.lock().unwrap().tag(),
    ])
    .rack(&mut rack);
    let r1 = rack.mono(1f32);
    u.lock().unwrap().set_active(&mut rack, 1.into());
    let r2 = rack.mono(1f32);
    u.lock().unwrap().set_active(&mut rack, 2.into());
    let r3 = rack.mono(1f32);
    assert_eq!((r1, r2, r3), (2.0, 3.0, 4.0));
}

#[test]
fn vca() {
    let mut rack = Rack::default();
    let c2 = ConstBuilder::new(2.0.into()).rack(&mut rack);
    let vca = VcaBuilder::new(c2.lock().unwrap().tag()).rack(&mut rack);
    vca.lock().unwrap().set_level(&mut rack, 2.5.into());
    let r = rack.mono(1f32);
    assert_eq!(r, 5.0);
}

#[test]
fn cross() {
    let mut rack = Rack::default();
    let c2 = ConstBuilder::new(2.0.into()).rack(&mut rack);
    let c3 = ConstBuilder::new(3.0.into()).rack(&mut rack);
    let cf =
        CrossFadeBuilder::new(c2.lock().unwrap().tag(), c3.lock().unwrap().tag()).rack(&mut rack);
    cf.lock().unwrap().set_alpha(&mut rack, 0.25.into());
    let r = rack.mono(1f32);
    assert_eq!(r, 2.25);
}

#[test]
fn modulator() {
    let mut rack = Rack::default();
    ModulatorBuilder::new(|_, _| 2.0)
        .hz(220.0)
        .ratio(2.0)
        .index(4.0)
        .rack(&mut rack);
    let r = rack.mono(1f32);
    assert_eq!(r, 3740.0);
}
