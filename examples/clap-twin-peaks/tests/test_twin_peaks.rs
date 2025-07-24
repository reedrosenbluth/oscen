use clack_host::factory::PluginFactory;
use clack_host::prelude::*;
use clap_twin_peaks::clap_entry;

#[test]
fn test_plugin_descriptor() {
    let _info = HostInfo::new("test", "", "", "").unwrap();

    let bundle = unsafe { PluginBundle::load_from_raw(&clap_entry, "") }.unwrap();

    let descriptor = bundle
        .get_factory::<PluginFactory>()
        .unwrap()
        .plugin_descriptor(0)
        .unwrap();

    assert_eq!(
        descriptor.id().unwrap().to_bytes(),
        b"org.rust-audio.clack.twin-peaks"
    );
    assert_eq!(descriptor.name().unwrap().to_bytes(), b"Twin Peaks Synth");
}
