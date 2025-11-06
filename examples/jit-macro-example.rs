// Example: What the automatic JIT macro would generate
//
// This shows what code would be automatically generated from a simple
// #[jit_node] macro usage, demonstrating that users can get JIT compilation
// without any knowledge of Cranelift.

// ═══════════════════════════════════════════════════════════════════════
// WHAT THE USER WRITES (simple, clean, no JIT knowledge needed)
// ═══════════════════════════════════════════════════════════════════════

#[jit_node]  // ← Single attribute enables automatic JIT!
pub struct OnePoleFilter {
    #[input(stream)]
    input: f32,

    #[input(value)]
    cutoff: f32,

    #[output(stream)]
    output: f32,

    // Internal state (not an input/output)
    state: f32,

    // The process method - user writes normal Rust code!
    fn process(&mut self, _sample_rate: f32, ctx: &mut ProcessingContext) -> f32 {
        let input = self.get_input(ctx);
        let cutoff = self.get_cutoff(ctx);

        // Simple lowpass filter: state += (input - state) * cutoff
        self.state += (input - self.state) * cutoff;

        self.state  // Return value
    }
}

// That's ALL the user writes! Everything below is AUTO-GENERATED:

// ═══════════════════════════════════════════════════════════════════════
// WHAT THE MACRO GENERATES (automatically, behind the scenes)
// ═══════════════════════════════════════════════════════════════════════

// 1. IO Struct (for struct-of-arrays pattern)
pub struct OnePoleFilterIO {
    pub input: f32,   // Stream inputs go in IO struct
    pub output: f32,  // Stream outputs go in IO struct
    // Note: cutoff (value input) stays in params, not IO
}

// 2. Endpoints struct (for type-safe connections)
pub struct OnePoleFilterEndpoints {
    pub node_key: NodeKey,
    pub input: StreamInput,
    pub cutoff: ValueInput,
    pub output: StreamOutput,
}

// 3. ProcessingNode trait implementation
impl ProcessingNode for OnePoleFilter {
    type Endpoints = OnePoleFilterEndpoints;

    const ENDPOINT_DESCRIPTORS: &'static [EndpointDescriptor] = &[
        EndpointDescriptor::new("input", EndpointType::Stream, EndpointDirection::Input),
        EndpointDescriptor::new("cutoff", EndpointType::Value, EndpointDirection::Input),
        EndpointDescriptor::new("output", EndpointType::Stream, EndpointDirection::Output),
    ];

    fn create_endpoints(
        node_key: NodeKey,
        inputs: &[ValueKey],
        outputs: &[ValueKey],
    ) -> Self::Endpoints {
        OnePoleFilterEndpoints {
            node_key,
            input: StreamInput { node_key, endpoint_key: inputs[0] },
            cutoff: ValueInput { node_key, endpoint_key: inputs[1] },
            output: StreamOutput { node_key, endpoint_key: outputs[0] },
        }
    }
}

// 4. Helper methods for accessing inputs
impl OnePoleFilter {
    fn get_input(&self, context: &ProcessingContext) -> f32 {
        context.get_stream_input(0)
    }

    fn get_cutoff(&self, context: &ProcessingContext) -> f32 {
        context.get_value_input(1)
    }
}

// 5. SignalProcessor implementation (interpreted mode)
impl SignalProcessor for OnePoleFilter {
    fn process(&mut self, _sample_rate: f32, context: &mut ProcessingContext) -> f32 {
        // Create IO struct
        let mut io = OnePoleFilterIO {
            input: self.get_input(context),
            output: 0.0,
        };

        // Get parameters
        let cutoff = self.get_cutoff(context);

        // Process (user's original code)
        self.state += (io.input - self.state) * cutoff;
        io.output = self.state;

        io.output
    }

    // Enable JIT compilation
    fn as_jit_codegen(&self) -> Option<&dyn JITCodegen> {
        Some(self)
    }
}

// 6. JITCodegen implementation (AUTOMATICALLY GENERATED from process() method!)
impl JITCodegen for OnePoleFilter {
    fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
        // The macro analyzed the process() method and translated it to Cranelift IR!

        // From: let input = self.get_input(ctx);
        let input = ctx.load_io(0);

        // From: let cutoff = self.get_cutoff(ctx);
        let cutoff = ctx.load_param(0);

        // From: let state = self.state;
        let state = ctx.load_state(0);

        // From: input - self.state
        let diff = ctx.builder.ins().fsub(input, state);

        // From: diff * cutoff
        let delta = ctx.builder.ins().fmul(diff, cutoff);

        // From: self.state += delta
        let new_state = ctx.builder.ins().fadd(state, delta);

        // From: self.state = new_state (assignment)
        ctx.store_state(new_state, 0);

        // From: return self.state (output)
        ctx.store_io(new_state, 4);

        Ok(())
    }

    fn jit_state_size(&self) -> usize {
        // Computed from struct: one f32 field (state)
        std::mem::size_of::<f32>()  // 4 bytes
    }

    fn jit_io_size(&self) -> usize {
        // Computed from struct: input + output
        std::mem::size_of::<f32>() * 2  // 8 bytes
    }

    fn jit_param_count(&self) -> usize {
        // Computed from struct: one value input (cutoff)
        1
    }

    fn jit_io_field_offsets(&self) -> HashMap<usize, usize> {
        // Computed from IO struct layout
        let mut offsets = HashMap::new();
        offsets.insert(0, 0);  // input at offset 0
        offsets.insert(1, 4);  // output at offset 4
        offsets
    }
}

// ═══════════════════════════════════════════════════════════════════════
// MACRO TRANSLATION EXAMPLES
// ═══════════════════════════════════════════════════════════════════════

// Example 1: Simple arithmetic
mod example1 {
    // User writes:
    fn user_code(a: f32, b: f32) -> f32 {
        a * b + a
    }

    // Macro generates:
    fn generated_jit_code(ctx: &mut CodegenContext, a: Value, b: Value) -> Value {
        let tmp0 = ctx.builder.ins().fmul(a, b);
        let result = ctx.builder.ins().fadd(tmp0, a);
        result
    }
}

// Example 2: State updates
mod example2 {
    // User writes:
    fn user_code(&mut self, input: f32) {
        self.state += input;
    }

    // Macro generates:
    fn generated_jit_code(ctx: &mut CodegenContext, input: Value) {
        let state = ctx.load_state(0);
        let new_state = ctx.builder.ins().fadd(state, input);
        ctx.store_state(new_state, 0);
    }
}

// Example 3: Math functions
mod example3 {
    // User writes:
    fn user_code(x: f32) -> f32 {
        x.abs().sqrt()
    }

    // Macro generates:
    fn generated_jit_code(ctx: &mut CodegenContext, x: Value) -> Value {
        let abs_x = ctx.builder.ins().fabs(x);
        let result = ctx.builder.ins().fsqrt(abs_x);
        result
    }
}

// Example 4: Constants
mod example4 {
    // User writes:
    fn user_code(phase: f32) -> f32 {
        phase * std::f32::consts::TAU
    }

    // Macro generates:
    fn generated_jit_code(ctx: &mut CodegenContext, phase: Value) -> Value {
        let tau = ctx.f32_const(std::f32::consts::TAU);
        let result = ctx.builder.ins().fmul(phase, tau);
        result
    }
}

// Example 5: Compound expressions
mod example5 {
    // User writes:
    fn user_code(&mut self, input: f32, freq: f32, sr: f32) {
        self.phase += freq / sr;
        self.phase %= 1.0;
    }

    // Macro generates:
    fn generated_jit_code(ctx: &mut CodegenContext, input: Value, freq: Value, sr: Value) {
        // self.phase += freq / sr
        let phase = ctx.load_state(0);
        let delta = ctx.builder.ins().fdiv(freq, sr);
        let new_phase = ctx.builder.ins().fadd(phase, delta);

        // self.phase %= 1.0
        let one = ctx.f32_const(1.0);
        let wrapped_phase = ctx.builder.ins().frem(new_phase, one);

        ctx.store_state(wrapped_phase, 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// COMPARISON: Before and After
// ═══════════════════════════════════════════════════════════════════════

// BEFORE (manual JIT codegen): User writes ~150 lines
// ----------------------------------------------------------------
// 1. Struct definition: 10 lines
// 2. Node derive: 1 line
// 3. SignalProcessor impl: 30 lines
// 4. JITCodegen impl: 80 lines
//    - emit_ir with all Cranelift calls
//    - jit_state_size
//    - jit_io_size
//    - jit_param_count
//    - jit_io_field_offsets
// 5. Helper methods: 20 lines
// Total: ~150 lines, requires Cranelift knowledge

// AFTER (automatic macro): User writes ~25 lines
// ----------------------------------------------------------------
// 1. #[jit_node] attribute: 1 line
// 2. Struct definition: 10 lines
// 3. Process method: 10 lines
// Total: ~25 lines, NO Cranelift knowledge needed!

// BENEFIT: 83% less code, 100% less complexity!

// ═══════════════════════════════════════════════════════════════════════
// USAGE: User perspective
// ═══════════════════════════════════════════════════════════════════════

fn example_usage() {
    use oscen::jit::JITGraph;

    // User just creates their node normally:
    let mut graph = JITGraph::new(44100.0);

    let filter = graph.add_node(OnePoleFilter {
        input: 0.0,
        cutoff: 0.5,
        output: 0.0,
        state: 0.0,
    });

    // That's it! The node is automatically JIT compiled!
    // User doesn't need to know:
    // - What Cranelift is
    // - How IR emission works
    // - Memory layout details
    // - Code generation
    //
    // They just write normal Rust code and get 10-20x speedup for free! 🎉

    let output = graph.process().unwrap();
}

// ═══════════════════════════════════════════════════════════════════════
// MACRO IMPLEMENTATION SKETCH
// ═══════════════════════════════════════════════════════════════════════

#[proc_macro_attribute]
pub fn jit_node(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // 1. Parse the struct definition
    let struct_def = parse_struct(item);

    // 2. Extract fields with attributes
    let inputs = extract_inputs(&struct_def);
    let outputs = extract_outputs(&struct_def);
    let state_fields = extract_state_fields(&struct_def);

    // 3. Find the process method
    let process_method = extract_process_method(&struct_def);

    // 4. Analyze process method AST
    let ast = parse_process_body(&process_method);

    // 5. Generate code
    let generated = quote! {
        // Original struct
        #struct_def

        // Generated IO struct
        #(generate_io_struct(inputs, outputs))

        // Generated endpoints
        #(generate_endpoints(inputs, outputs))

        // ProcessingNode impl
        #(generate_processing_node_impl())

        // Helper methods
        #(generate_helper_methods(inputs))

        // SignalProcessor impl (interpreted)
        #(generate_signal_processor_impl(process_method))

        // JITCodegen impl (JIT compiled!)
        #(generate_jit_codegen_impl(ast, inputs, outputs, state_fields))
    };

    generated.into()
}

// The magic happens in generate_jit_codegen_impl:
fn generate_jit_codegen_impl(
    ast: ProcessAst,
    inputs: Vec<Field>,
    outputs: Vec<Field>,
    state_fields: Vec<Field>,
) -> TokenStream {
    // Walk the AST and emit Cranelift IR generation code
    let ir_emission = ast.statements.iter().map(|stmt| {
        match stmt {
            Statement::Let { name, value } => {
                translate_let_binding(name, value)
            }
            Statement::Assign { target, value } => {
                translate_assignment(target, value)
            }
            Statement::Expr(expr) => {
                translate_expression(expr)
            }
        }
    });

    quote! {
        impl JITCodegen for #struct_name {
            fn emit_ir(&self, ctx: &mut CodegenContext) -> Result<(), CodegenError> {
                #(#ir_emission)*
                Ok(())
            }

            // Auto-computed sizes
            fn jit_state_size(&self) -> usize {
                #(compute_state_size(state_fields))
            }

            fn jit_io_size(&self) -> usize {
                #(compute_io_size(inputs, outputs))
            }

            fn jit_param_count(&self) -> usize {
                #(count_value_inputs(inputs))
            }
        }
    }
}

// Example translation:
fn translate_expression(expr: &Expr) -> TokenStream {
    match expr {
        Expr::Binary { left, op, right } => {
            let left_code = translate_expression(left);
            let right_code = translate_expression(right);

            let cranelift_op = match op {
                BinOp::Add => quote! { fadd },
                BinOp::Sub => quote! { fsub },
                BinOp::Mul => quote! { fmul },
                BinOp::Div => quote! { fdiv },
                _ => panic!("Unsupported operator"),
            };

            quote! {
                {
                    let left_val = #left_code;
                    let right_val = #right_code;
                    ctx.builder.ins().#cranelift_op(left_val, right_val)
                }
            }
        }
        // ... handle other expression types
    }
}
