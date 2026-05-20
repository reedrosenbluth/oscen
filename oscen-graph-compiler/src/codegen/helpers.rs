//! Naming conventions and type-name builders used by codegen emitters.
//!
//! These are pure functions with no dependency on `CodegenContext` —
//! they take the few inputs they need and return token streams or idents.

use crate::ast::ConnectionPolicy;
use crate::ir::graph::EdgeKernel;
use crate::ir::expr::{IrExpr, IrExprKind};
use crate::ir::graph::{EventRescale, IrGraph};
use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

/// Field name for the resampler kernel state stored on the graph struct for
/// the connection at `idx` (index into `IrGraph::edge_order`).
pub(super) fn resampler_field_name(idx: usize) -> Ident {
    syn::Ident::new(
        &format!("__resampler_{}", idx),
        proc_macro2::Span::call_site(),
    )
}

/// Local-variable name for the upsample buffer associated with edge `idx`.
pub(super) fn up_buf_name(idx: usize) -> Ident {
    syn::Ident::new(&format!("__up_buf_{}", idx), proc_macro2::Span::call_site())
}

/// Local-variable name for the downsample accumulator buffer associated with
/// edge `idx`.
pub(super) fn down_buf_name(idx: usize) -> Ident {
    syn::Ident::new(
        &format!("__down_buf_{}", idx),
        proc_macro2::Span::call_site(),
    )
}

/// Map a parsed `ConnectionPolicy` to the marker-type token used in
/// `CrossRateKernel<_, _, Policy, _, _>` projections.
pub(super) fn policy_marker_path(policy: ConnectionPolicy) -> TokenStream {
    match policy {
        ConnectionPolicy::Default => quote! { ::oscen::dispatch::DefaultPolicy },
        ConnectionPolicy::Sinc => quote! { ::oscen::dispatch::SincPolicy },
        ConnectionPolicy::SincIir => quote! { ::oscen::dispatch::SincIirPolicy },
        ConnectionPolicy::Linear => quote! { ::oscen::dispatch::LinearPolicy },
        ConnectionPolicy::Latch => quote! { ::oscen::dispatch::LatchPolicy },
    }
}

/// Choose the Rust kernel type for an upsampler edge based on policy.
pub(super) fn kernel_up_type(factor: u32, policy: ConnectionPolicy) -> TokenStream {
    let n = factor as usize;
    match policy {
        ConnectionPolicy::Latch => quote! { ::oscen::resample::LatchUp<#n> },
        ConnectionPolicy::Linear => quote! { ::oscen::resample::LinearUp<#n> },
        ConnectionPolicy::Sinc | ConnectionPolicy::Default => {
            quote! { ::oscen::resample::SincUpFir<#n> }
        }
        ConnectionPolicy::SincIir => quote! { ::oscen::resample::IirHalfbandUp<#n> },
    }
}

/// Choose the Rust kernel type for a downsampler edge based on policy.
pub(super) fn kernel_down_type(factor: u32, policy: ConnectionPolicy) -> TokenStream {
    let n = factor as usize;
    match policy {
        ConnectionPolicy::Latch => quote! { ::oscen::resample::LatchDown<#n> },
        ConnectionPolicy::Linear => quote! { ::oscen::resample::LinearDown<#n> },
        ConnectionPolicy::Sinc | ConnectionPolicy::Default => {
            quote! { ::oscen::resample::SincDownFir<#n> }
        }
        ConnectionPolicy::SincIir => quote! { ::oscen::resample::IirHalfbandDown<#n> },
    }
}

/// True for edges that flow through the same-rate `ConnectEndpoints` path:
/// either a true same-rate edge or a same-rate event edge. Cross-rate event
/// edges have their own dedicated rescale path and are not handled here.
pub(super) fn is_same_rate_kernel(k: &EdgeKernel) -> bool {
    matches!(
        k,
        EdgeKernel::None
            | EdgeKernel::Event {
                rescale: EventRescale::None
            }
    )
}

/// Extract the root node name from an IR expression (the leftmost node's name
/// as a String). Walks through Binary/MethodCall to find the leftmost
/// Endpoint variant. Returns None for Call/Literal.
pub(super) fn root_node_name(expr: &IrExpr, ir: &IrGraph) -> Option<String> {
    match &expr.kind {
        IrExprKind::Endpoint(ep) => Some(ir.nodes[ep.node].name.to_string()),
        IrExprKind::Binary { left, .. } => root_node_name(left, ir),
        IrExprKind::MethodCall { receiver, .. } => root_node_name(receiver, ir),
        IrExprKind::Call { .. } | IrExprKind::Literal(_) => None,
    }
}

/// Compute the greatest common divisor of two numbers.
pub(super) fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Compute the least common multiple of two numbers.
pub(super) fn lcm(a: u32, b: u32) -> u32 {
    a / gcd(a, b) * b
}
