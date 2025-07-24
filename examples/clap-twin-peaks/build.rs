use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    
    // Bundle configuration
    let bundle_name = "TwinPeaks.clap";
    let bundle_identifier = "com.oscen.twin-peaks.clap";
    let bundle_version = "0.1.0";
    let dylib_name = if cfg!(target_os = "macos") {
        "libclap_twin_peaks.dylib"
    } else if cfg!(target_os = "windows") {
        "clap_twin_peaks.dll"
    } else {
        "libclap_twin_peaks.so"
    };
    
    // Create bundle structure
    let target_dir = project_root().join("target").join(&profile);
    let bundle_dir = target_dir.join(bundle_name);
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    
    // Create directories
    fs::create_dir_all(&macos_dir)?;
    
    // Create Info.plist
    let info_plist = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>TwinPeaks</string>
    <key>CFBundleIdentifier</key>
    <string>{}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Twin Peaks Synth</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>{}</string>
    <key>CFBundleVersion</key>
    <string>{}</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.13</string>
</dict>
</plist>"#, bundle_identifier, bundle_version, bundle_version);
    
    fs::write(contents_dir.join("Info.plist"), info_plist)?;
    fs::write(contents_dir.join("PkgInfo"), "BNDL????")?;
    
    // Copy dylib
    let src_dylib = target_dir.join(dylib_name);
    let dst_dylib = macos_dir.join("TwinPeaks");
    
    if src_dylib.exists() {
        fs::copy(&src_dylib, &dst_dylib)?;
        println!("Copied dylib from {:?} to {:?}", src_dylib, dst_dylib);
    } else {
        println!("Dylib not found at {:?}, will be copied on next build", src_dylib);
    }
    
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}