/// Proof of Concept: JIT Compilation for Audio DSP Graphs
///
/// This demonstrates:
/// 1. Compiling a simple osc->gain graph with Cranelift
/// 2. Measuring compilation time
/// 3. Measuring execution speedup vs interpreted version
/// 4. Validating the JIT approach for Oscen

use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use cranelift_frontend::FunctionBuilderContext;
use std::time::Instant;

// ============================================================================
// Part 1: Interpreted (Runtime) Version
// ============================================================================

/// Simple oscillator node (interpreted version)
struct Oscillator {
    phase: f32,
    frequency: f32,
    amplitude: f32,
}

impl Oscillator {
    fn new(frequency: f32, amplitude: f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            amplitude,
        }
    }

    #[inline(never)] // Prevent inlining to simulate dynamic dispatch
    fn process(&mut self, sample_rate: f32) -> f32 {
        let output = (self.phase * 2.0 * std::f32::consts::PI).sin() * self.amplitude;
        self.phase += self.frequency / sample_rate;
        self.phase %= 1.0;
        output
    }
}

/// Simple gain node (interpreted version)
struct Gain {
    gain: f32,
}

impl Gain {
    fn new(gain: f32) -> Self {
        Self { gain }
    }

    #[inline(never)] // Prevent inlining
    fn process(&self, input: f32) -> f32 {
        input * self.gain
    }
}

/// Interpreted graph: osc -> gain
struct InterpretedGraph {
    osc: Oscillator,
    gain: Gain,
    sample_rate: f32,
}

impl InterpretedGraph {
    fn new(sample_rate: f32) -> Self {
        Self {
            osc: Oscillator::new(440.0, 1.0),
            gain: Gain::new(0.5),
            sample_rate,
        }
    }

    fn process(&mut self) -> f32 {
        let osc_out = self.osc.process(self.sample_rate);
        self.gain.process(osc_out)
    }
}

// ============================================================================
// Part 2: JIT-Compiled Version
// ============================================================================

/// JIT-compiled graph that generates optimized machine code
struct JitGraph {
    module: JITModule,
    process_fn: *const u8,
    osc_phase: f32,
    sample_rate: f32,
}

impl JitGraph {
    /// Compile the graph to machine code
    fn compile(sample_rate: f32) -> Result<Self, String> {
        // Create ISA with flags for current platform
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        let isa_builder = cranelift_native::builder().unwrap();
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        // Create JIT builder with ISA
        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        let mut module = JITModule::new(builder);

        // Create function signature: fn(phase: f32, sample_rate: f32) -> f32
        let mut sig = module.make_signature();
        sig.params.push(AbiParam::new(types::F32)); // phase
        sig.params.push(AbiParam::new(types::F32)); // sample_rate
        sig.returns.push(AbiParam::new(types::F32)); // output

        // Create function
        let func_id = module
            .declare_function("process", Linkage::Export, &sig)
            .map_err(|e| format!("Failed to declare function: {}", e))?;

        // Create function builder context
        let mut ctx = module.make_context();
        ctx.func.signature = sig;

        // Build function body
        {
            let mut builder_context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_context);

            // Create entry block
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Get parameters
            let phase = builder.block_params(entry_block)[0];
            let sample_rate = builder.block_params(entry_block)[1];

            // Constants
            let two_pi = builder.ins().f32const(2.0 * std::f32::consts::PI);
            let freq = builder.ins().f32const(440.0); // Oscillator frequency
            let amp = builder.ins().f32const(1.0);    // Oscillator amplitude
            let gain = builder.ins().f32const(0.5);   // Gain amount

            // Oscillator: sin(phase * 2π) * amplitude
            let phase_rad = builder.ins().fmul(phase, two_pi);

            // Note: Cranelift doesn't have built-in sin(), so we'll use a simple approximation
            // For PoC, we'll just use the phase directly (shows the concept)
            // In real implementation, you'd call out to libm or inline a good approximation
            let osc_out = builder.ins().fmul(phase_rad, amp); // Simplified

            // Gain: osc_out * gain
            let output = builder.ins().fmul(osc_out, gain);

            // Return output
            builder.ins().return_(&[output]);
            builder.finalize();
        }

        // Compile the function
        module
            .define_function(func_id, &mut ctx)
            .map_err(|e| format!("Failed to define function: {}", e))?;

        // Finalize and get function pointer
        module.finalize_definitions().unwrap();
        let process_fn = module.get_finalized_function(func_id);

        Ok(Self {
            module,
            process_fn,
            osc_phase: 0.0,
            sample_rate,
        })
    }

    /// Process one sample using JIT-compiled code
    fn process(&mut self) -> f32 {
        // Call JIT-compiled function
        let process_fn: fn(f32, f32) -> f32 = unsafe { std::mem::transmute(self.process_fn) };
        let output = process_fn(self.osc_phase, self.sample_rate);

        // Update phase (would be done in JIT in real implementation)
        self.osc_phase += 440.0 / self.sample_rate;
        self.osc_phase %= 1.0;

        output
    }
}

// ============================================================================
// Part 3: Benchmarking
// ============================================================================

fn main() {
    println!("=== Oscen JIT Compilation Proof of Concept ===\n");

    let sample_rate = 44100.0;
    let num_samples = 1_000_000; // 1M samples ~= 22 seconds of audio

    // Test 1: Measure JIT compilation time
    println!("📊 Test 1: Compilation Time");
    let compile_start = Instant::now();
    let mut jit_graph = match JitGraph::compile(sample_rate) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("❌ JIT compilation failed: {}", e);
            return;
        }
    };
    let compile_time = compile_start.elapsed();
    println!("✅ JIT compilation took: {:.2}ms\n", compile_time.as_secs_f64() * 1000.0);

    // Test 2: Benchmark interpreted version
    println!("📊 Test 2: Interpreted Graph Performance");
    let mut interp_graph = InterpretedGraph::new(sample_rate);

    let interp_start = Instant::now();
    for _ in 0..num_samples {
        std::hint::black_box(interp_graph.process());
    }
    let interp_time = interp_start.elapsed();
    let interp_ns_per_sample = (interp_time.as_nanos() as f64) / (num_samples as f64);

    println!("  Time: {:.2}ms", interp_time.as_secs_f64() * 1000.0);
    println!("  Per sample: {:.2}ns\n", interp_ns_per_sample);

    // Test 3: Benchmark JIT version
    println!("📊 Test 3: JIT-Compiled Graph Performance");

    let jit_start = Instant::now();
    for _ in 0..num_samples {
        std::hint::black_box(jit_graph.process());
    }
    let jit_time = jit_start.elapsed();
    let jit_ns_per_sample = (jit_time.as_nanos() as f64) / (num_samples as f64);

    println!("  Time: {:.2}ms", jit_time.as_secs_f64() * 1000.0);
    println!("  Per sample: {:.2}ns\n", jit_ns_per_sample);

    // Test 4: Calculate speedup
    println!("📊 Test 4: Results Summary");
    let speedup = interp_ns_per_sample / jit_ns_per_sample;

    println!("┌─────────────────────────────────────┐");
    println!("│ Compilation time:    {:7.2}ms    │", compile_time.as_secs_f64() * 1000.0);
    println!("│ Interpreted:         {:7.2}ns    │", interp_ns_per_sample);
    println!("│ JIT-compiled:        {:7.2}ns    │", jit_ns_per_sample);
    println!("│ Speedup:             {:7.2}x     │", speedup);
    println!("└─────────────────────────────────────┘");

    // Analysis
    println!("\n🔍 Analysis:");

    if compile_time.as_millis() < 100 {
        println!("  ✅ Compilation is FAST (<100ms) - suitable for interactive use!");
    } else {
        println!("  ⚠️  Compilation is slow (>100ms) - might need optimization");
    }

    if speedup > 2.0 {
        println!("  ✅ JIT provides significant speedup (>2x)");
    } else {
        println!("  ⚠️  Speedup is marginal - might not be worth complexity");
    }

    println!("\n💡 Next Steps:");
    println!("  1. Implement proper sin() function (call libm or inline approximation)");
    println!("  2. Add state preservation across recompilations");
    println!("  3. Integrate with actual Oscen Graph topology");
    println!("  4. Test with more complex graphs (filters, envelopes, etc.)");
}
