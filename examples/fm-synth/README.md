# FM Synth

A 3-operator FM synthesizer plugin built with Oscen and NIH-plug.

## Features

- **8-voice polyphony** with voice allocation
- **3 FM operators** with configurable routing
- **Per-operator ADSR envelopes** for amplitude shaping
- **Self-feedback** on modulator operators (OP2, OP3)
- **Routing crossfade** blends OP3 between modulating OP2 or OP1
- **TPT state-variable filter** with envelope modulation

## Architecture

```
OP3 (modulator)
  │
  ├──[route=0]──► OP2 (modulator) ──► OP1 (carrier) ──► Filter ──► Output
  │                                     ▲
  └──[route=1]──────────────────────────┘
```

## Building

```bash
cargo xtask bundle fm-synth --release
```

This produces VST3 and CLAP plugins in `target/bundled/`.

