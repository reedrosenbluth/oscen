# Plan: Add Explicit Input/Output Type Declarations (CMajor-Style)

## Goal
Make the graph macro require explicit type declarations for all graph-level inputs/outputs, similar to CMajor's approach. Node-to-node connections will infer types from these declared endpoints.

## CMajor Pattern (Reference)
```cmajor
graph ElectricPiano {
    input event std::midi::Message midiIn;
    output stream float<2> audioOut;

    node voices = Voice[16];
    node allocator = std::voices::VoiceAllocator(16);

    connection {
        midiIn -> allocator.eventIn;
        allocator.voiceEventOut -> voices.eventIn;
        voices.audioOut -> audioOut;
    }
}
```

## Proposed Oscen Syntax Enhancement

### Current (Ambiguous)
```rust
graph! {
    input event midi_in;  // Type is implicit
    output stream left_out;
}
```

### Proposed (Explicit)
```rust
graph! {
    input midi_in: event;  // Explicit type declaration
    output left_out: stream;
    output right_out: stream;

    // OR with Rust-style syntax:
    input midi_in: Event;
    output left_out: Stream;
}
```

## Implementation Plan

### 1. Update Parser to Accept Type Syntax
**File**: `oscen-macros/src/graph_macro/parse.rs`
**Location**: Lines 81-121 (input/output parsing)

**Current**:
```rust
"input" => {
    let kind = tokens.parse::<Ident>()?;  // "event", "value", "stream"
    let name = tokens.parse::<Ident>()?;
    // ...
}
```

**Enhanced**:
```rust
"input" => {
    let name = tokens.parse::<Ident>()?;
    tokens.parse::<Token![:]>()?;  // Expect colon
    let kind = tokens.parse::<Ident>()?;  // "event", "value", "stream"
    // ...
}
```

Support both syntaxes for backward compatibility during transition:
```rust
"input" => {
    let first = tokens.parse::<Ident>()?;

    // Check if next token is colon (new syntax) or another ident (old syntax)
    if tokens.peek(Token![:]) {
        // New syntax: name: type
        tokens.parse::<Token![:]>()?;
        let kind = tokens.parse::<Ident>()?;
        // name=first, kind=parsed
    } else {
        // Old syntax: type name
        let kind = first;
        let name = tokens.parse::<Ident>()?;
        // Warn about deprecation
    }
}
```

### 2. Remove Type Inference for Unconnected Endpoints
**File**: `oscen-macros/src/graph_macro/codegen.rs`
**Modify**: `infer_node_endpoint_types()` at lines 159-193

**Current behavior**: Tries to infer types even without complete information

**New behavior**: Only propagate types from declared graph inputs/outputs
- If graph input type is declared → propagate to connected nodes ✅
- If both endpoints unknown → ERROR with clear message ❌

```rust
// After type propagation, check for unregistered endpoints
for conn in &self.connections {
    let source_type = type_ctx.infer_type(&conn.source);
    let dest_type = type_ctx.infer_type(&conn.dest);

    if source_type.is_none() {
        return Err(syn::Error::new_spanned(
            &conn.source,
            format!("Cannot determine type for endpoint. Ensure it's connected to a graph input/output with explicit type.")
        ));
    }
    if dest_type.is_none() {
        return Err(syn::Error::new_spanned(
            &conn.dest,
            format!("Cannot determine type for endpoint. Ensure it's connected to a graph input/output with explicit type.")
        ));
    }
}
```

### 3. Update Electric-Piano to Use Explicit Types

**File**: `examples/electric-piano/src/main.rs`

**Before**:
```rust
graph! {
    name: ElectricPianoGraph;
    compile_time: true;

    input value brightness = 30.0;
    // ... other inputs
}
```

**After**:
```rust
graph! {
    name: ElectricPianoGraph;
    compile_time: true;

    // Explicit type declarations
    input brightness: value = 30.0;
    input velocity_scaling: value = 50.0;
    // ... other inputs

    output left_out: stream;
    output right_out: stream;
}
```

### 4. Add Type Declaration for Internal Event Flow

For MIDI input that isn't a graph-level input:

**Option A**: Add graph-level MIDI input
```rust
graph! {
    input midi_in: event;  // Explicit event input

    connections {
        midi_in -> midi_parser.input;  // Type flows from here
        midi_parser.note_on -> voice_allocator.note_on;  // Inferred
    }
}
```

**Option B**: Use explicit intermediate connections
```rust
graph! {
    // If MIDI comes from runtime, create internal event bus
    nodes {
        midi_parser = MidiParser::new();  // Has event outputs
    }

    connections {
        // Type inferred from MidiParser's declared output types
        midi_parser.note_on -> voice_allocator.note_on;
    }
}
```

### 5. Document Type Flow Rules

**File**: Create `docs/GRAPH_MACRO_TYPES.md`

```markdown
# Graph Macro Type System

## Type Declaration Rules

1. **Graph inputs/outputs MUST have explicit types**
   - `input name: event`
   - `output name: stream`
   - `input name: value = default`

2. **Node endpoints inherit types through connections**
   - If `graph_input -> node.endpoint`, node.endpoint inherits graph_input's type
   - If `node1.out -> node2.in`, types must match

3. **Type inference requires connection to declared type**
   - All endpoints must trace back to a graph input/output
   - Or infer from node's known endpoint types (if available)

## Error Messages

- "Cannot infer type for X: no connection to typed endpoint"
- "Type mismatch: event -> stream not allowed"
- "Endpoint X is not connected to any typed source"
```

## Migration Path

### Phase 1: Make Both Syntaxes Work (This PR)
- Parse both `input event name` and `input name: event`
- Emit deprecation warnings for old syntax
- Update electric-piano to new syntax

### Phase 2: Deprecation Period (Next Release)
- Old syntax works but warns loudly
- Update all examples
- Update documentation

### Phase 3: Remove Old Syntax (Future Release)
- Only accept `name: type` syntax
- Clean error messages for old syntax

## Benefits

✅ **No Ambiguity**: All types explicitly declared at graph boundary
✅ **CMajor Alignment**: Matches proven pattern from CMajor
✅ **Better Errors**: Clear what's wrong when types don't match
✅ **Self-Documenting**: Graph declaration shows all I/O types
✅ **Type Safety**: Compiler can verify type flow
✅ **No Heuristics**: No guessing or pattern matching

## Testing Plan

1. Update parser tests for new syntax
2. Test backward compatibility with old syntax
3. Update electric-piano example
4. Verify type propagation works correctly
5. Test error messages for type mismatches
6. Ensure voice allocator example compiles

## Files to Modify

1. `oscen-macros/src/graph_macro/parse.rs` - Parser updates (~50 lines)
2. `oscen-macros/src/graph_macro/codegen.rs` - Error handling (~30 lines)
3. `examples/electric-piano/src/main.rs` - Update syntax (~10 lines)
4. `docs/GRAPH_MACRO_TYPES.md` - New documentation
5. Update other examples to use new syntax (optional but recommended)

## Timeline Estimate

- Parser changes: 1-2 hours
- Error handling: 30 minutes
- Electric-piano updates: 15 minutes
- Testing: 1 hour
- **Total: ~3-4 hours**

## Current Status (Pre-Implementation)

### What's Working Now
- ✅ VoiceAllocator refactored with EventContext trait
- ✅ Static graphs create StaticContext with pending_events
- ✅ Array event routing infrastructure in place
- ✅ PendingEvent includes array_index field
- ✅ derive(Node) macro handles const generics

### What Needs This Fix
- ❌ Graph macro can't detect event endpoints with `()` placeholder types
- ❌ Electric-piano won't compile with `compile_time: true`
- ❌ Connections like `voice_allocator.note_on -> voice_handlers.note_on` fail

### Why This Happens
The graph! macro cannot read Node's ENDPOINT_DESCRIPTORS at macro expansion time because:
1. Proc macros run before type resolution
2. Cannot evaluate const trait items
3. Relies on type inference from connections
4. When both endpoints unknown, inference fails → no event storage generated

### The Solution
By requiring explicit types at graph boundaries, we create "anchors" that type inference can propagate from:
```
graph input (known) → node1.endpoint (inferred) → node2.endpoint (inferred)
```

This is exactly how CMajor handles it, and it's proven to work well in practice.
