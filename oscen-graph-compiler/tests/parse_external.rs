use oscen_graph_compiler::ast::{EndpointKind, GraphItem};
use oscen_graph_compiler::parse::parse_graph_def;
use oscen_graph_compiler::Diagnostics;

/// Task 4b: a graph declaring `external ir: AudioAsset;` and binding it with
/// `ir -> reverb.ir;` must parse without diagnostics. The external is captured
/// as a `GraphItem::External` carrying its name and declared type.
#[test]
fn external_decl_and_asset_connection_parse_without_diagnostics() {
    let toks: proc_macro2::TokenStream = "name: ReverbRenderGraph; \
         input stream dry; \
         output stream wet; \
         external ir: AudioAsset; \
         node reverb = Convolver::new(); \
         connections { dry -> reverb.input; reverb.output -> wet; ir -> reverb.ir; }"
        .parse()
        .unwrap();

    let mut diags = Diagnostics::new();
    let parsed = parse_graph_def(toks, &mut diags);
    assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags);

    let ext = parsed
        .items
        .iter()
        .find_map(|item| match item {
            GraphItem::External(decl) => Some(decl),
            _ => None,
        })
        .expect("external decl");
    assert_eq!(ext.name.to_string(), "ir");

    // The declared type is `AudioAsset`.
    let ty = &ext.ty;
    let ty_str = quote::quote!(#ty).to_string();
    assert_eq!(ty_str, "AudioAsset");
}

/// Task 4b: the `asset` keyword resolves to `EndpointKind::Asset` through the
/// endpoint-kind parser.
#[test]
fn asset_keyword_parses_to_asset_endpoint_kind() {
    let kind: EndpointKind = syn::parse_str("asset").expect("asset should parse as endpoint kind");
    assert_eq!(kind, EndpointKind::Asset);
}
