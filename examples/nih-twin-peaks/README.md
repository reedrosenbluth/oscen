# Twin Peaks NIH Plugin

## Building

From the project root directory:

```bash
# Build CLAP and VST3 plugins in release mode
cargo run --package xtask --release -- bundle twin-peaks-nih --release
```

This will create both formats in the `target/bundled/` directory:
- `twin-peaks-nih.clap` - CLAP plugin
- `twin-peaks-nih.vst3` - VST3 plugin

## Installation

### macOS
```bash
# CLAP Plugin
cp -r target/bundled/twin-peaks-nih.clap ~/Library/Audio/Plug-Ins/CLAP/

# VST3 Plugin
cp -r target/bundled/twin-peaks-nih.vst3 ~/Library/Audio/Plug-Ins/VST3/
```

### Linux
- CLAP: `~/.clap/` or `/usr/lib/clap/`
- VST3: `~/.vst3/` or `/usr/lib/vst3/`

### Windows
- CLAP: `%COMMONPROGRAMFILES%\CLAP\`
- VST3: `%COMMONPROGRAMFILES%\VST3\`
