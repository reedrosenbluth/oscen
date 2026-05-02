//! Implementation of the `oversample_variants!` macro.
//!
//! Materializes multiple compile-time `graph!` variants from a single body,
//! substituting an integer factor for each occurrence of the placeholder
//! token `{FACTOR}` in the body.
//!
//! # Example
//! ```ignore
//! oversample_variants! {
//!     base_name: MyGraph;
//!     factors: [1, 2, 4];
//!     body: {
//!         output stream audio_out;
//!         nodes {
//!             osc = PolyBlepOscillator::saw(440.0, 0.6) * {FACTOR};
//!         }
//!         connections {
//!             [sinc] osc.output -> audio_out;
//!         }
//!     }
//! }
//! ```
//!
//! Generates `MyGraph_1x`, `MyGraph_2x`, and `MyGraph_4x` graph types.

use proc_macro2::{Delimiter, Group, Ident, TokenStream, TokenTree};
use quote::quote;
use syn::{
    braced, bracketed,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    LitInt, Result, Token,
};

mod kw {
    syn::custom_keyword!(base_name);
    syn::custom_keyword!(factors);
    syn::custom_keyword!(body);
}

/// Parsed input to `oversample_variants!`.
struct OversampleVariantsInput {
    base_name: Ident,
    factors: Vec<u32>,
    body: TokenStream,
}

impl Parse for OversampleVariantsInput {
    fn parse(input: ParseStream) -> Result<Self> {
        // base_name: Ident;
        input.parse::<kw::base_name>()?;
        input.parse::<Token![:]>()?;
        let base_name: Ident = input.parse()?;
        input.parse::<Token![;]>()?;

        // factors: [<int>, <int>, ...];
        input.parse::<kw::factors>()?;
        input.parse::<Token![:]>()?;
        let factors_content;
        bracketed!(factors_content in input);
        let factor_lits: Punctuated<LitInt, Token![,]> =
            factors_content.parse_terminated(LitInt::parse, Token![,])?;
        let mut factors = Vec::with_capacity(factor_lits.len());
        for lit in factor_lits {
            factors.push(lit.base10_parse::<u32>()?);
        }
        if factors.is_empty() {
            return Err(syn::Error::new(
                input.span(),
                "`factors` list must contain at least one factor",
            ));
        }
        input.parse::<Token![;]>()?;

        // body: { ... }
        input.parse::<kw::body>()?;
        input.parse::<Token![:]>()?;
        let body_content;
        braced!(body_content in input);
        let body: TokenStream = body_content.parse()?;

        // Optional trailing semicolon.
        let _ = input.parse::<Token![;]>();

        Ok(OversampleVariantsInput {
            base_name,
            factors,
            body,
        })
    }
}

/// Entry point invoked from the proc-macro shim in `lib.rs`.
pub fn oversample_variants_impl(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let parsed = match syn::parse::<OversampleVariantsInput>(input) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut output = TokenStream::new();
    for factor in &parsed.factors {
        let variant_name = Ident::new(
            &format!("{}_{}x", parsed.base_name, factor),
            parsed.base_name.span(),
        );
        let body = substitute_factor(parsed.body.clone(), *factor);
        output.extend(quote! {
            ::oscen::graph! {
                name: #variant_name;
                #body
            }
        });
    }
    output.into()
}

/// Recursively walk the token stream replacing every `{FACTOR}` placeholder
/// (a brace-delimited group containing only the ident `FACTOR`) with the
/// integer literal `factor`.
fn substitute_factor(input: TokenStream, factor: u32) -> TokenStream {
    let mut out = TokenStream::new();
    for tt in input.into_iter() {
        match tt {
            TokenTree::Group(g) if is_factor_placeholder(&g) => {
                let mut lit = proc_macro2::Literal::u32_unsuffixed(factor);
                lit.set_span(g.span());
                out.extend(std::iter::once(TokenTree::Literal(lit)));
            }
            TokenTree::Group(g) => {
                let inner = substitute_factor(g.stream(), factor);
                let mut new_group = Group::new(g.delimiter(), inner);
                new_group.set_span(g.span());
                out.extend(std::iter::once(TokenTree::Group(new_group)));
            }
            other => out.extend(std::iter::once(other)),
        }
    }
    out
}

/// Returns true iff `g` is exactly `{FACTOR}` — a brace group containing
/// only the single identifier `FACTOR`.
fn is_factor_placeholder(g: &Group) -> bool {
    if g.delimiter() != Delimiter::Brace {
        return false;
    }
    let mut iter = g.stream().into_iter();
    let first = iter.next();
    let second = iter.next();
    match (first, second) {
        (Some(TokenTree::Ident(i)), None) => i == "FACTOR",
        _ => false,
    }
}

