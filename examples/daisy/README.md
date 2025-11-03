# Oscen Daisy Example

A basic example demonstrating how to run an Oscen audio patch on the Electrosmith Daisy Seed hardware platform.

## What This Example Does

This example creates a simple audio patch that outputs a 220Hz (A3) sine wave through a low-pass filter. The Daisy's USER LED blinks to indicate the program is running.

The audio graph consists of:
- **Sine Oscillator**: 220Hz, 0.3 amplitude
- **TPT Low-Pass Filter**: 1200Hz cutoff, 0.707 Q (resonance)

## Hardware Requirements

- Electrosmith Daisy Seed
- Audio output (headphones or speakers connected to Daisy's audio out)
- USB cable for programming

## Software Requirements

### 1. Install Rust with ARM Support

```bash
# Install rustup if you haven't already
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add ARM Cortex-M7 target
rustup target add thumbv7em-none-eabihf
```

### 2. Install Flashing Tool

Choose one of the following methods:

#### Option A: DFU-Util (Recommended for Beginners)

This uses Daisy's built-in USB bootloader.

**macOS:**
```bash
brew install dfu-util
```

**Linux (Debian/Ubuntu):**
```bash
sudo apt-get install dfu-util
```

**Linux (Arch):**
```bash
sudo pacman -S dfu-util
```

#### Option B: Probe-Run (Recommended for Development)

This requires a debug probe (ST-Link, J-Link, etc.) but provides better debugging.

```bash
cargo install probe-run
```

#### Option C: OpenOCD

```bash
# macOS
brew install openocd

# Linux (Debian/Ubuntu)
sudo apt-get install openocd
```

## Building the Example

From this directory (`examples/daisy`), run:

```bash
cargo build --release
```

The compiled binary will be at: `target/thumbv7em-none-eabihf/release/oscen-daisy`

## Flashing to Daisy

### Method 1: DFU-Util (USB Bootloader)

1. **Put Daisy into bootloader mode:**
   - Hold the BOOT button on the Daisy Seed
   - Press and release the RESET button
   - Release the BOOT button
   - The Daisy is now in bootloader mode

2. **Flash the binary:**

```bash
# Convert ELF to binary format
cargo objcopy --release -- -O binary oscen-daisy.bin

# Flash using dfu-util
dfu-util -a 0 -s 0x08000000:leave -D oscen-daisy.bin
```

Or use the helper script (if you uncomment the dfu-util runner in `.cargo/config.toml`):

```bash
cargo run --release
```

### Method 2: Probe-Run (Debug Probe)

1. **Connect your debug probe** to the Daisy's SWD pins
2. **Uncomment the probe-run runner** in `.cargo/config.toml`
3. **Flash:**

```bash
cargo run --release
```

### Method 3: OpenOCD + GDB

1. **Start OpenOCD** in one terminal:

```bash
openocd -f interface/stlink.cfg -f target/stm32h7x.cfg
```

2. **Flash using GDB** in another terminal:

```bash
arm-none-eabi-gdb target/thumbv7em-none-eabihf/release/oscen-daisy
(gdb) target remote :3333
(gdb) monitor reset halt
(gdb) load
(gdb) monitor reset run
(gdb) quit
```

## Verifying It Works

Once flashed:
1. The USER LED should blink approximately once per second
2. You should hear a 220Hz tone from the audio output
3. The tone will be filtered by a low-pass filter at 1200Hz

## Customizing the Patch

Edit `src/main.rs` to create your own audio patch:

```rust
// Example: Change to a sawtooth wave at 440Hz
let osc = graph.add_node(Oscillator::saw(440.0, 0.3));

// Example: Add more modules
let osc2 = graph.add_node(Oscillator::sine(110.0, 0.2));
let mixer = graph.add_node(/* ... */);
graph.connect(osc.output, mixer.input_a);
graph.connect(osc2.output, mixer.input_b);
```

See the oscen-lib documentation for available modules:
- **Oscillators**: `Oscillator::sine()`, `Oscillator::saw()`, `PolyBlepOscillator`
- **Filters**: `TptFilter`, `OnePoleFilter`
- **Envelopes**: `AdsrEnvelope`
- **Effects**: `Delay`, `Gain`

## Troubleshooting

### Build Errors

**"can't find crate for `std`"**
- This is a `no_std` environment. Make sure oscen-lib doesn't use any std-only features
- Check that all dependencies support `no_std`

**Linker errors about memory regions**
- The daisy_bsp crate should provide `memory.x`
- Make sure `link.x` is being generated correctly

### Flashing Errors

**"Cannot open DFU device"**
- Make sure Daisy is in bootloader mode (see instructions above)
- On Linux, you may need udev rules:
  ```bash
  # Create /etc/udev/rules.d/50-daisy.rules
  SUBSYSTEM=="usb", ATTR{idVendor}=="0483", ATTR{idProduct}=="df11", MODE="0666"

  # Reload rules
  sudo udevadm control --reload-rules
  sudo udevadm trigger
  ```

**"No probe found"**
- Check that your debug probe is connected
- Try: `probe-run --list-probes`

**No audio output**
- Check your audio connections
- Verify the volume is turned up
- Try a different oscillator frequency to ensure it's not outside your hearing range

### Runtime Issues

**LED not blinking**
- The program may have crashed or not started
- Try reflashing or check the serial output if using probe-run

**No sound or distorted audio**
- Check the amplitude values (they should be between -1.0 and 1.0)
- Verify the sample rate matches Daisy's configuration (48kHz)

## Memory Constraints

The Daisy Seed has:
- **128KB DTCM RAM** (tightly coupled, very fast)
- **512KB RAM** (AXI SRAM)
- **64MB SDRAM** (external, slower)

Complex patches with many nodes may need optimization:
- Use fixed-size buffers (`arrayvec`)
- Pre-allocate nodes at startup
- Avoid allocations in the audio callback
- Consider using SDRAM for delay lines or large buffers

## Next Steps

- **Add CV inputs**: Read control voltage from Daisy's ADC pins
- **Add knobs**: Map potentiometers to oscillator frequency or filter cutoff
- **Add gates/triggers**: Use GPIO for note triggers
- **Polyphony**: Create multiple voices with the voice allocator
- **Effects**: Add delay, reverb, or other effects modules

## Resources

- [Daisy BSP Documentation](https://github.com/antoinevg/daisy_bsp)
- [Oscen Documentation](https://github.com/yourusername/oscen)
- [Electrosmith Daisy Wiki](https://github.com/electro-smith/DaisyWiki)
- [Embedded Rust Book](https://rust-embedded.github.io/book/)

## License

This example is provided under the same license as the Oscen project.
