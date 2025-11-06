# Automatic JIT Codegen via Macros

## Vision: Zero-Configuration JIT

Users should write normal node implementations and get JIT compilation **for free**:

```rust
#[derive(Debug, Node)]
pub struct MyFilter {
    #[input(stream)]
    input: f32,

    #[input(value)]
    cutoff: f32,

    #[output(stream)]
    output: f32,

    state: f32,
}

impl SignalProcessor for MyFilter {
    fn process(&mut self, _sr: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let cutoff = self.get_cutoff(ctx);

        self.state += (input - self.state) * cutoff;
        self.state
    }
}

// That's it! JIT codegen is AUTOMATICALLY generated!
```

No `JITCodegen` trait implementation needed. No Cranelift knowledge required. Just works.

## How It Works

### Step 1: Capture Process Method in Macro

Extend `#[derive(Node)]` to accept the `impl SignalProcessor` block:

```rust
#[derive(Debug, Node)]
#[jit] // Opt-in to automatic JIT codegen
pub struct MyFilter {
    #[input(stream)] input: f32,
    #[input(value)] cutoff: f32,
    #[output(stream)] output: f32,
    state: f32,
}

// The macro captures this entire impl block!
impl SignalProcessor for MyFilter {
    fn process(&mut self, _sr: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let cutoff = self.get_cutoff(ctx);
        self.state += (input - self.state) * cutoff;
        self.state
    }
}
```

**Problem:** Rust macros can't normally capture `impl` blocks.

**Solution:** Use a combined macro:

```rust
#[jit_node]
pub struct MyFilter {
    #[input(stream)] input: f32,
    #[input(value)] cutoff: f32,
    #[output(stream)] output: f32,
    state: f32,

    // Process method defined inline!
    fn process(&mut self, _sr: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let cutoff = self.get_cutoff(ctx);
        self.state += (input - self.state) * cutoff;
        self.state
    }
}
```

The `#[jit_node]` macro:
1. Parses the struct definition
2. Extracts the `process` method
3. Generates both `Node` derive and `JITCodegen` impl
4. Generates helper methods (get_input, etc.)

### Step 2: Parse Process Method AST

The macro analyzes the `process()` method body:

```rust
fn process(&mut self, _sr: f32, ctx: &mut ProcessingContext) -> f32 {
    let input = self.get_input(ctx);           // → Load IO
    let cutoff = self.get_cutoff(ctx);         // → Load param
    self.state += (input - self.state) * cutoff;  // → Math ops + state update
    self.state                                 // → Return state
}
```

**AST Structure:**
```
Block:
  - Let statement: input = self.get_input(ctx)
  - Let statement: cutoff = self.get_cutoff(ctx)
  - Assignment: self.state += (expression)
  - Return: self.state
```

### Step 3: Translate to Cranelift IR Emission

The macro generates code that emits Cranelift IR:

```rust
// Generated JITCodegen implementation:
impl JITCodegen for MyFilter {
    fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // Translated from: let input = self.get_input(ctx);
        let input = ctx.load_io(0);

        // Translated from: let cutoff = self.get_cutoff(ctx);
        let cutoff = ctx.load_param(0);

        // Translated from: let state = self.state;
        let state = ctx.load_state(0);

        // Translated from: input - self.state
        let tmp0 = ctx.builder.ins().fsub(input, state);

        // Translated from: tmp0 * cutoff
        let tmp1 = ctx.builder.ins().fmul(tmp0, cutoff);

        // Translated from: self.state += tmp1
        let new_state = ctx.builder.ins().fadd(state, tmp1);

        // Translated from: self.state = new_state
        ctx.store_state(new_state, 0);

        // Translated from: return self.state
        ctx.store_io(new_state, 4);

        Ok(())
    }

    // Auto-computed from struct definition
    fn jit_state_size(&self) -> usize { 4 }
    fn jit_io_size(&self) -> usize { 8 }
    fn jit_param_count(&self) -> usize { 1 }
}
```

## Translation Rules

### Pattern Matching for Common Operations

The macro recognizes common patterns and translates them:

#### 1. Loading Inputs/Parameters

```rust
// User writes:
let input = self.get_input(ctx);

// Macro recognizes: get_<field_name>(ctx)
// Knows "input" is #[input(stream)] from struct definition
// Translates to:
let input = ctx.load_io(offset_of_input);
```

#### 2. Arithmetic Operations

```rust
// User writes:
let result = a + b * c;

// Macro translates to:
let tmp0 = ctx.builder.ins().fmul(b, c);
let result = ctx.builder.ins().fadd(a, tmp0);
```

**Translation table:**
| Rust | Cranelift |
|------|-----------|
| `a + b` | `ctx.builder.ins().fadd(a, b)` |
| `a - b` | `ctx.builder.ins().fsub(a, b)` |
| `a * b` | `ctx.builder.ins().fmul(a, b)` |
| `a / b` | `ctx.builder.ins().fdiv(a, b)` |
| `a % b` | `ctx.builder.ins().frem(a, b)` |
| `-a` | `ctx.builder.ins().fneg(a)` |
| `a.abs()` | `ctx.builder.ins().fabs(a)` |
| `a.sqrt()` | `ctx.builder.ins().fsqrt(a)` |
| `a.min(b)` | `ctx.builder.ins().fmin(a, b)` |
| `a.max(b)` | `ctx.builder.ins().fmax(a, b)` |

#### 3. State Updates

```rust
// User writes:
self.state = new_value;

// Macro recognizes: assignment to self.<field>
// Knows "state" is not #[input] or #[output]
// Translates to:
ctx.store_state(new_value, offset_of_state);
```

#### 4. Compound Assignment

```rust
// User writes:
self.state += delta;

// Macro translates to:
let state = ctx.load_state(0);
let new_state = ctx.builder.ins().fadd(state, delta);
ctx.store_state(new_state, 0);
```

#### 5. Method Calls

```rust
// User writes:
let clamped = value.clamp(0.0, 1.0);

// Macro translates to:
let min = ctx.f32_const(0.0);
let max = ctx.f32_const(1.0);
let clamped = ctx.builder.ins().fmin(ctx.builder.ins().fmax(value, min), max);
```

#### 6. Constants

```rust
// User writes:
let two_pi = std::f32::consts::TAU;

// Macro translates to:
let two_pi = ctx.f32_const(std::f32::consts::TAU);
```

### Handling Unsupported Patterns

If the macro encounters code it can't translate, it has options:

**Option 1: Compile Error**
```rust
// User writes:
if input > 0.5 {  // Complex control flow!
    return input;
} else {
    return 0.0;
}

// Macro emits:
compile_error!("Conditional logic in process() is not supported for JIT compilation.
                Consider using select() or implementing JITCodegen manually.");
```

**Option 2: Partial JIT**
```rust
// User writes complex code
fn process(&mut self, _sr: f32, ctx: &mut ProcessingContext) -> f32 {
    let input = self.get_input(ctx);
    // ... JIT-able code ...

    if complex_condition {  // Not JIT-able!
        // complex logic
    }

    // ... more JIT-able code ...
}

// Macro generates:
// - No JITCodegen implementation (falls back to interpreted)
// - Emits warning:
#[warn("process() contains control flow that cannot be JIT compiled.
        Node will run in interpreted mode.")]
```

**Option 3: Split Implementation**
```rust
// Macro generates both:
impl SignalProcessor for MyNode {
    fn process(&mut self, sr: f32, ctx: &mut ProcessingContext) -> f32 {
        // Original implementation (interpreted)
    }
}

impl JITCodegen for MyNode {
    fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // Generated JIT code (only simple parts)
        // OR returns UnsupportedOperation error
    }
}
```

## Implementation Plan

### Phase 1: Basic Arithmetic (Week 1)

Support simple arithmetic nodes:

```rust
#[jit_node]
pub struct Gain {
    #[input(stream)] input: f32,
    #[input(value)] gain: f32,
    #[output(stream)] output: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let gain = self.get_gain(ctx);
        input * gain  // Simple arithmetic
    }
}
```

**Supports:**
- Load inputs/parameters
- Basic arithmetic (+, -, *, /)
- Return output

### Phase 2: State Updates (Week 2)

Support stateful nodes:

```rust
#[jit_node]
pub struct Integrator {
    #[input(stream)] input: f32,
    #[output(stream)] output: f32,
    state: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        self.state += input;  // State update
        self.state
    }
}
```

**Adds:**
- State field detection
- Load state
- Update state
- Compound assignments (+=, -=, *=, /=)

### Phase 3: Math Functions (Week 3)

Support math operations:

```rust
#[jit_node]
pub struct Oscillator {
    #[input(value)] frequency: f32,
    #[output(stream)] output: f32,
    phase: f32,
    sample_rate: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let freq = self.get_frequency(ctx);
        self.phase += freq / self.sample_rate;
        self.phase = self.phase % 1.0;
        (self.phase * std::f32::consts::TAU).sin()  // Math functions
    }
}
```

**Adds:**
- sin, cos, tan
- sqrt, abs
- min, max, clamp
- Constants (PI, TAU, etc.)

### Phase 4: Arrays and Loops (Week 4-5)

Support fixed-size arrays:

```rust
#[jit_node]
pub struct Biquad {
    #[input(stream)] input: f32,
    #[output(stream)] output: f32,

    // Coefficients
    b0: f32, b1: f32, b2: f32,
    a1: f32, a2: f32,

    // State
    x1: f32, x2: f32,
    y1: f32, y2: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);

        // Biquad difference equation
        let output =
            self.b0 * input +
            self.b1 * self.x1 +
            self.b2 * self.x2 -
            self.a1 * self.y1 -
            self.a2 * self.y2;

        // Update state
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;

        output
    }
}
```

**Adds:**
- Multiple state fields
- Sequential updates
- Complex expressions

### Phase 5: Limited Control Flow (Week 6)

Support simple conditionals via select:

```rust
#[jit_node]
pub struct Clipper {
    #[input(stream)] input: f32,
    #[input(value)] threshold: f32,
    #[output(stream)] output: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let threshold = self.get_threshold(ctx);

        // Translate to select instruction
        input.clamp(-threshold, threshold)
    }
}
```

**Adds:**
- clamp, min, max (via select)
- Simple ternary-like patterns

## Advanced: Expression Analysis

The macro performs **dataflow analysis** on the process method:

```rust
fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
    let a = self.get_input(ctx);      // Load from IO
    let b = self.get_param(ctx);      // Load from params
    let c = self.state;               // Load from state

    let tmp1 = a * b;                 // Compute
    let tmp2 = tmp1 + c;              // Compute

    self.state = tmp2;                // Store to state
    tmp2                              // Return (store to IO)
}
```

**Dataflow graph:**
```
IO[input] ──┐
            ├─→ mul ─→ tmp1 ─┐
Param[0] ───┘                ├─→ add ─→ tmp2 ──┬─→ State[0]
                             │                  │
State[0] ────────────────────┘                  └─→ IO[output]
```

The macro can optimize:
- Eliminate dead code
- Reorder operations
- Reuse values
- Minimize loads/stores

## User Experience

### Example 1: Simple Node

```rust
// User writes this:
#[jit_node]
pub struct Volume {
    #[input(stream)] input: f32,
    #[input(value)] volume: f32,
    #[output(stream)] output: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let volume = self.get_volume(ctx);
        input * volume
    }
}

// Macro automatically generates:
// - Node trait impl
// - JITCodegen trait impl
// - Helper methods
// - Memory layout info

// User just uses it:
let mut graph = JITGraph::new(44100.0);
let vol = graph.add_node(Volume::new(0.8));
// Automatically JIT compiled! 🎉
```

### Example 2: Complex Node (Auto-fallback)

```rust
// User writes complex logic:
#[jit_node]
pub struct WaveShaper {
    #[input(stream)] input: f32,
    #[output(stream)] output: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);

        // Complex waveshaping with branching
        if input > 0.5 {
            input.powi(2)  // powi not supported!
        } else if input < -0.5 {
            -(-input).powi(2)
        } else {
            input
        }
    }
}

// Macro detects unsupported patterns:
// - Generates warning at compile time
// - Automatically falls back to interpreted mode
// - Node still works, just not JIT compiled
```

### Example 3: Opt-out

```rust
// User can explicitly disable JIT:
#[jit_node(disable_jit)]
pub struct ComplexDSP {
    // ... complex implementation that shouldn't be JIT'd
}

// Or just use regular #[derive(Node)]:
#[derive(Debug, Node)]
pub struct LegacyNode {
    // ... old-style implementation
}
```

## Comparison to Manual Implementation

### Manual (Current):
```rust
// User writes 100+ lines:
// 1. Define struct with Node derive
// 2. Implement SignalProcessor
// 3. Implement JITCodegen
//    - emit_ir with all Cranelift calls
//    - jit_state_size
//    - jit_io_size
//    - jit_param_count
//    - jit_io_field_offsets
```

### Automatic (Proposed):
```rust
// User writes ~20 lines:
#[jit_node]
pub struct MyNode {
    #[input(stream)] input: f32,
    #[output(stream)] output: f32,

    fn process(&mut self, _: f32, ctx: &mut ProcessingContext) -> f32 {
        // Normal Rust code
    }
}

// Everything else is automatic!
```

**Reduction: 80% less code, 100% less Cranelift knowledge needed!**

## Implementation Complexity

### Difficulty: Medium-High

**Challenges:**
1. **Proc macro complexity**: Need to parse and analyze function bodies
2. **AST translation**: Map Rust operations to Cranelift IR
3. **Error handling**: Provide helpful messages for unsupported patterns
4. **Edge cases**: Handle all valid Rust code gracefully

**Existing solutions to study:**
- FAUST compiler (DSP → C++)
- Enzyme (automatic differentiation via LLVM)
- quote/syn crates for macro magic

### Timeline

- **Phase 1-2**: 2 weeks (basic arithmetic + state)
- **Phase 3**: 1 week (math functions)
- **Phase 4**: 2 weeks (arrays and complex expressions)
- **Phase 5**: 1 week (limited control flow)
- **Testing**: 1 week
- **Total**: ~7-8 weeks for full implementation

### Incremental Approach

Start simple, add features over time:

1. **Week 1-2**: Support only stateless arithmetic
   - Still provides value for Gain, Mix, etc.

2. **Week 3-4**: Add state support
   - Unlocks filters, envelopes

3. **Week 5-6**: Add math functions
   - Unlocks oscillators

4. **Week 7-8**: Polish and optimize
   - Better error messages
   - More patterns recognized

Each phase provides value independently!

## Decision: Worth It?

**Pros:**
- ✅ Best possible user experience
- ✅ Users get JIT for free
- ✅ No Cranelift knowledge required
- ✅ Automatic optimization
- ✅ Catches more nodes automatically

**Cons:**
- ⚠️ Complex macro implementation
- ⚠️ Longer development time
- ⚠️ Harder to debug generated code
- ⚠️ May not support all patterns

**Recommendation: YES, but phase it**

1. **Ship manual JITCodegen trait first** (already designed)
   - Gets basic JIT working
   - Provides immediate value
   - Users can manually implement JIT

2. **Add automatic macro later** (this design)
   - Better UX
   - Reduces friction
   - Covers 80% of nodes automatically

This gives us:
- Short-term: JIT that works (manual)
- Long-term: JIT that's delightful (automatic)

## Alternative: Hybrid Approach

**Best of both worlds:**

```rust
// Simple nodes: automatic
#[jit_node]
pub struct SimpleGain { /* ... */ }

// Complex nodes: manual
#[derive(Node)]
pub struct ComplexFilter { /* ... */ }

impl JITCodegen for ComplexFilter {
    // Hand-written for maximum control
}

// Interpreted nodes: neither
#[derive(Node)]
pub struct GranularSynth { /* ... */ }
// Just use SignalProcessor, no JIT
```

Users choose the right tool for each node!

## Conclusion

**Automatic JIT codegen via macros is definitely possible** and would provide the best UX.

**Recommended path:**
1. ✅ Implement manual JITCodegen trait (done in design)
2. ✅ Add built-in node registry (easy win)
3. ✅ Add interpreted fallback (enables mixing)
4. 🔄 Implement automatic macro (future enhancement)

This gives users **immediate value** while building toward the **ideal experience**.

The macro would make Oscen feel like FAUST or CMajor but with Rust's safety and ecosystem!
