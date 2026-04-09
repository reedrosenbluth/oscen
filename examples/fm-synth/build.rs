fn main() {
    let mut config = slint_build::CompilerConfiguration::new();
    if cfg!(not(feature = "standalone")) {
        config = config.embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer);
    }
    slint_build::compile_with_config("ui/synth_window.slint", config).unwrap();
}
