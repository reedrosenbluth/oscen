use crate::ast::*;
use syn::{
    braced, bracketed, parenthesized,
    parse::{Parse, ParseStream},
    token, Expr, Ident, Result, Token,
};

impl Parse for GraphDef {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut items: Vec<GraphItem> = Vec::new();
        while !input.is_empty() {
            items.push(input.parse()?);
        }

        let mut name = None;
        if matches!(items.first(), Some(GraphItem::Name(_))) {
            if let GraphItem::Name(n) = items.remove(0) {
                name = Some(n);
            }
        }

        // Any remaining Name variant is misplaced — `name:` must appear first.
        for item in &items {
            if let GraphItem::Name(n) = item {
                return Err(syn::Error::new(
                    n.span(),
                    "`name:` declaration must appear at the start of the graph",
                ));
            }
        }

        Ok(GraphDef { name, items })
    }
}

impl Parse for GraphItem {
    fn parse(input: ParseStream) -> Result<Self> {
        // `name: <ident>;` declaration — only valid as the first item;
        // `Parse for GraphDef` drains it into `GraphDef.name` and reports
        // an error if it appears later. We accept it here regardless so
        // that the parser doesn't bail on a stray misplaced `name:`.
        if input.peek(kw::name) && input.peek2(Token![:]) {
            input.parse::<kw::name>()?;
            input.parse::<Token![:]>()?;
            let name: Ident = input.parse()?;
            input.parse::<Token![;]>()?;
            return Ok(GraphItem::Name(name));
        }

        let lookahead = input.lookahead1();

        if lookahead.peek(kw::nih_params) {
            // Parse `nih_params;` statement
            input.parse::<kw::nih_params>()?;
            input.parse::<Token![;]>()?;
            Ok(GraphItem::NihParams)
        } else if lookahead.peek(kw::input) {
            Ok(GraphItem::Input(input.parse()?))
        } else if lookahead.peek(kw::output) {
            Ok(GraphItem::Output(input.parse()?))
        } else if lookahead.peek(kw::node) {
            // Check if it's a block or single declaration
            let fork = input.fork();
            fork.parse::<kw::node>()?;

            if fork.peek(token::Brace) {
                Ok(GraphItem::NodeBlock(input.parse()?))
            } else {
                Ok(GraphItem::Node(input.parse()?))
            }
        } else if lookahead.peek(kw::nodes) {
            Ok(GraphItem::NodeBlock(input.parse()?))
        } else if lookahead.peek(kw::connection) {
            // Check if it's a block or single statement
            let fork = input.fork();
            fork.parse::<kw::connection>()?;

            if fork.peek(token::Brace) {
                Ok(GraphItem::ConnectionBlock(input.parse()?))
            } else {
                Ok(GraphItem::Connection(input.parse()?))
            }
        } else if lookahead.peek(kw::connections) {
            Ok(GraphItem::ConnectionBlock(input.parse()?))
        } else {
            Err(lookahead.error())
        }
    }
}

/// Parse brace-style ParamSpec for NIH-plug parameters
/// Syntax: { range: 20.0..20000.0, center: 1000.0, unit: " Hz", smoother: 50.0, step: 0.5, display_name: "Cutoff", group: "Filter" }
fn parse_brace_param_spec(input: ParseStream) -> Result<ParamSpec> {
    let content;
    braced!(content in input);

    let mut range = None;
    let mut curve = None;
    let mut ramp = None;
    let mut center = None;
    let mut unit = None;
    let mut smoother = None;
    let mut step = None;
    let mut display_name = None;
    let mut group = None;

    // Parse comma-separated key: value pairs
    while !content.is_empty() {
        let lookahead = content.lookahead1();

        if lookahead.peek(kw::range) {
            content.parse::<kw::range>()?;
            content.parse::<Token![:]>()?;
            // Parse range expression: min..max
            // Use parse_simple_expr to avoid consuming the `..` as part of a range expression
            let min = parse_simple_expr(&content)?;
            content.parse::<Token![..]>()?;
            let max = parse_simple_expr(&content)?;
            range = Some(RangeSpec { min, max });
        } else if lookahead.peek(kw::center) {
            content.parse::<kw::center>()?;
            content.parse::<Token![:]>()?;
            center = Some(content.parse()?);
        } else if lookahead.peek(kw::unit) {
            content.parse::<kw::unit>()?;
            content.parse::<Token![:]>()?;
            let lit: syn::LitStr = content.parse()?;
            unit = Some(lit.value());
        } else if lookahead.peek(kw::smoother) {
            content.parse::<kw::smoother>()?;
            content.parse::<Token![:]>()?;
            smoother = Some(content.parse()?);
        } else if lookahead.peek(kw::step) {
            content.parse::<kw::step>()?;
            content.parse::<Token![:]>()?;
            step = Some(content.parse()?);
        } else if lookahead.peek(kw::name) {
            content.parse::<kw::name>()?;
            content.parse::<Token![:]>()?;
            let lit: syn::LitStr = content.parse()?;
            display_name = Some(lit.value());
        } else if lookahead.peek(kw::group) {
            content.parse::<kw::group>()?;
            content.parse::<Token![:]>()?;
            let lit: syn::LitStr = content.parse()?;
            group = Some(lit.value());
        } else if lookahead.peek(kw::linear) {
            content.parse::<kw::linear>()?;
            curve = Some(Curve::Linear);
        } else if lookahead.peek(kw::log) {
            content.parse::<kw::log>()?;
            curve = Some(Curve::Logarithmic);
        } else if lookahead.peek(kw::ramp) {
            content.parse::<kw::ramp>()?;
            content.parse::<Token![:]>()?;
            let lit: syn::LitInt = content.parse()?;
            ramp = Some(lit.base10_parse()?);
        } else {
            return Err(lookahead.error());
        }

        // Optional trailing comma
        if content.peek(Token![,]) {
            content.parse::<Token![,]>()?;
        }
    }

    Ok(ParamSpec {
        range,
        curve,
        ramp,
        center,
        unit,
        smoother,
        step,
        display_name,
        group,
    })
}

impl Parse for InputDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::input>()?;

        // Try to parse: either "name: kind" (new CMajor-style) or "kind name" (old style)
        let first_ident = input.parse::<Ident>()?;

        let (name, kind) = if input.peek(Token![:]) {
            // NEW SYNTAX: input name: kind
            input.parse::<Token![:]>()?;
            let kind = input.parse::<EndpointKind>()?;
            (first_ident, kind)
        } else {
            // OLD SYNTAX: input kind name
            // Parse first_ident as EndpointKind
            let kind = parse_endpoint_kind_from_ident(&first_ident)?;
            let name = input.parse::<Ident>()?;
            (name, kind)
        };

        // Parse optional type annotation: `: Type` (for array types like [f32; 32])
        // This is a SECOND colon for the new syntax: input name: event: [Type; N]
        // For old syntax it's the first: input event name: [Type; N]
        let ty = if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        let mut default = None;
        let mut spec = None;

        // Parse optional default value
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            // Parse a literal or simple expression, but stop at `[`, `{`, or `;`
            // We can't use parse::<Expr>() because it will consume the `[...]` as array indexing
            default = Some(parse_simple_expr(input)?);

            // Parse optional parameter spec in brackets or braces
            if input.peek(token::Bracket) {
                spec = Some(input.parse()?);
            } else if input.peek(token::Brace) {
                spec = Some(parse_brace_param_spec(input)?);
            }
        }

        input.parse::<Token![;]>()?;

        Ok(InputDecl {
            kind,
            name,
            ty,
            default,
            spec,
        })
    }
}

/// Parse optional `* N` or `/ N` after a node constructor expression, where N is a
/// power-of-2 integer literal in {1, 2, 4, 8}. Stops at `;`.
fn parse_node_rate(input: ParseStream) -> Result<NodeRate> {
    if input.peek(Token![;]) {
        return Ok(NodeRate::Same);
    }
    let is_up = if input.peek(Token![*]) {
        input.parse::<Token![*]>()?;
        true
    } else if input.peek(Token![/]) {
        input.parse::<Token![/]>()?;
        false
    } else {
        return Ok(NodeRate::Same);
    };
    let lit: syn::LitInt = input.parse()?;
    let n: u32 = lit.base10_parse()?;
    if !matches!(n, 1 | 2 | 4 | 8) {
        return Err(syn::Error::new(
            lit.span(),
            "rate factor must be 1, 2, 4, or 8",
        ));
    }
    Ok(if n == 1 {
        NodeRate::Same
    } else if is_up {
        NodeRate::Up(n)
    } else {
        NodeRate::Down(n)
    })
}

/// Walk down the left side of nested `Binary(Mul|Div, _, IntLit)` expressions
/// until we stop being a rate-binary. Returns `true` if the chain bottoms out
/// at an `Expr::Repeat`.
fn rate_chain_ends_in_repeat(expr: &Expr) -> bool {
    use syn::{BinOp, ExprLit, Lit};
    let mut cursor = expr;
    while let Expr::Binary(bin) = cursor {
        let is_rate_op = matches!(bin.op, BinOp::Mul(_) | BinOp::Div(_));
        let rhs_is_int = matches!(
            &*bin.right,
            Expr::Lit(ExprLit {
                lit: Lit::Int(_),
                ..
            })
        );
        if is_rate_op && rhs_is_int {
            cursor = &*bin.left;
        } else {
            return false;
        }
    }
    matches!(cursor, Expr::Repeat(_))
}

/// Post-process a parsed constructor expression to extract:
///   - the actual constructor expression to store in NodeDecl
///   - the array size if the constructor was `[expr; N]`
///   - an embedded rate if the constructor was `[expr; N] * M` or `[expr; N] / M`
///
/// Recognised shapes (in order):
///   0. Expr::Binary(Mul|Div, lhs=rate-chain ending in Repeat, rhs=IntLit)
///                                                    → Err (conflict: `[X; N] * M * P`)
///   1. Expr::Binary(Mul|Div, lhs=Repeat, rhs=IntLit) → (inner, size, Some(rate))
///   2. Expr::Repeat                                  → (inner, size, None)
///   3. anything else                                 → (expr,  None,  None)
fn extract_array_and_embedded_rate(expr: Expr) -> Result<(Expr, Option<usize>, Option<NodeRate>)> {
    use syn::{BinOp, ExprLit, Lit};

    // Helper: unwrap `Expr::Repeat` into (inner, count_opt). Caller has
    // already checked `expr` is a Repeat.
    fn unwrap_repeat(repeat: syn::ExprRepeat) -> Result<(Expr, Option<usize>)> {
        let count = if let Expr::Lit(ExprLit {
            lit: Lit::Int(c), ..
        }) = &*repeat.len
        {
            Some(c.base10_parse::<usize>()?)
        } else {
            None
        };
        Ok((*repeat.expr, count))
    }

    // Shape 1: Binary(Mul|Div, Repeat, IntLit) → embedded rate
    if let Expr::Binary(bin) = &expr {
        let is_up = matches!(bin.op, BinOp::Mul(_));
        let is_down = matches!(bin.op, BinOp::Div(_));

        // Shape 0: nested rate chain on the left → conflict.
        // E.g. `[X; 4] * 2 * 4` parses as `Binary(Mul, Binary(Mul, Repeat, 2), 4)`.
        // Walk left through any Mul|Div * IntLit chain; if we land on a Repeat,
        // the user wrote multiple rate factors and we should report a conflict.
        if (is_up || is_down) && matches!(&*bin.left, Expr::Binary(_)) {
            if let Expr::Lit(ExprLit {
                lit: Lit::Int(n_lit),
                ..
            }) = &*bin.right
            {
                if rate_chain_ends_in_repeat(&bin.left) {
                    return Err(syn::Error::new(
                        n_lit.span(),
                        "node already has an embedded rate (`* N` or `/ N`) from the array literal; \
                         remove the trailing rate annotation",
                    ));
                }
            }
        }

        if (is_up || is_down) && matches!(&*bin.left, Expr::Repeat(_)) {
            if let Expr::Lit(ExprLit {
                lit: Lit::Int(n_lit),
                ..
            }) = &*bin.right
            {
                let n: u32 = n_lit.base10_parse()?;
                if !matches!(n, 1 | 2 | 4 | 8) {
                    return Err(syn::Error::new(
                        n_lit.span(),
                        "rate factor must be 1, 2, 4, or 8",
                    ));
                }
                let rate = if n == 1 {
                    NodeRate::Same
                } else if is_up {
                    NodeRate::Up(n)
                } else {
                    NodeRate::Down(n)
                };
                // Re-destructure to get owned values out of `expr`.
                let Expr::Binary(bin) = expr else {
                    unreachable!()
                };
                let Expr::Repeat(repeat) = *bin.left else {
                    unreachable!()
                };
                let (inner, count) = unwrap_repeat(repeat)?;
                return Ok((inner, count, Some(rate)));
            }
        }
    }

    // Shape 2: bare Repeat
    if let Expr::Repeat(repeat) = expr {
        let (inner, count) = unwrap_repeat(repeat)?;
        return Ok((inner, count, None));
    }

    // Shape 3: anything else
    Ok((expr, None, None))
}

// Parse a simple expression that won't consume brackets
fn parse_simple_expr(input: ParseStream) -> Result<Expr> {
    // Parse literals (numbers, strings, etc.) or simple paths
    // Stop when we see `[` or `;`
    // DON'T use parse::<Expr>() because it will consume brackets as array indexing!
    if input.peek(syn::LitFloat)
        || input.peek(syn::LitInt)
        || input.peek(syn::LitStr)
        || input.peek(syn::LitBool)
    {
        let lit: syn::Lit = input.parse()?;
        Ok(Expr::Lit(syn::ExprLit { attrs: vec![], lit }))
    } else if input.peek(Ident) {
        // Could be a path like std::f32::consts::PI
        Ok(Expr::Path(input.parse()?))
    } else if input.peek(Token![-]) {
        // Negative number
        input.parse::<Token![-]>()?;
        let lit: syn::Lit = input.parse()?;
        Ok(Expr::Unary(syn::ExprUnary {
            attrs: vec![],
            op: syn::UnOp::Neg(Default::default()),
            expr: Box::new(Expr::Lit(syn::ExprLit { attrs: vec![], lit })),
        }))
    } else {
        Err(input.error("expected literal or identifier for default value"))
    }
}

impl Parse for OutputDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::output>()?;

        // Try to parse: either "name: kind" (new CMajor-style) or "kind name" (old style)
        let first_ident = input.parse::<Ident>()?;

        let (name, kind) = if input.peek(Token![:]) {
            // NEW SYNTAX: output name: kind
            input.parse::<Token![:]>()?;
            let kind = input.parse::<EndpointKind>()?;
            (first_ident, kind)
        } else {
            // OLD SYNTAX: output kind name
            // Parse first_ident as EndpointKind
            let kind = parse_endpoint_kind_from_ident(&first_ident)?;
            let name = input.parse::<Ident>()?;
            (name, kind)
        };

        // Parse optional type annotation: `: Type` (for array types)
        // This is a SECOND colon for the new syntax
        let ty = if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        input.parse::<Token![;]>()?;

        Ok(OutputDecl { kind, name, ty })
    }
}

/// Parse the body of a node declaration (everything after the `node`
/// keyword): `<name> = <constructor> [* N | / N];`. Shared between
/// `Parse for NodeDecl` (which consumes `node` first) and the block
/// chunker in Task 4 (which slices brace contents into body chunks).
fn parse_node_decl_body(input: ParseStream) -> Result<NodeDecl> {
    let name = input.parse()?;
    input.parse::<Token![=]>()?;
    let (constructor, extracted_type) = parse_constructor_with_type(input)?;
    let (actual_constructor, array_size, embedded_rate) =
        extract_array_and_embedded_rate(constructor)?;
    let node_type = extracted_type.or_else(|| extract_node_type(&actual_constructor));

    let rate = match embedded_rate {
        Some(r) => {
            if !input.peek(Token![;]) {
                return Err(input.error(
                    "node already has an embedded rate (`* N` or `/ N`) from the array literal; \
                     remove the trailing rate annotation",
                ));
            }
            r
        }
        None => parse_node_rate(input)?,
    };

    input.parse::<Token![;]>()?;

    Ok(NodeDecl {
        name,
        constructor: actual_constructor,
        node_type,
        array_size,
        rate,
    })
}

impl Parse for NodeDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::node>()?;
        parse_node_decl_body(input)
    }
}

// Parse node block
fn parse_node_block(input: ParseStream) -> Result<Vec<NodeDecl>> {
    // Accept either 'node' or 'nodes'
    if input.peek(kw::nodes) {
        input.parse::<kw::nodes>()?;
    } else {
        input.parse::<kw::node>()?;
    }

    let content;
    braced!(content in input);

    let mut nodes = Vec::new();
    while !content.is_empty() {
        nodes.push(parse_node_decl_body(&content)?);
    }

    Ok(nodes)
}

/// Parse a constructor expression, handling generic type parameters
/// Returns both the expression and the extracted type (if found)
/// This supports syntax like `Type<N>::new()` in addition to `Type::new()`
fn parse_constructor_with_type(input: ParseStream) -> Result<(Expr, Option<syn::Path>)> {
    use proc_macro2::TokenTree;
    use quote::quote;
    use syn::parse::discouraged::Speculative;

    // Fork to manually check for Type<...>::method() pattern
    let fork = input.fork();

    // Try to manually parse the generic type pattern
    // This avoids the ambiguity issue with < being a comparison operator
    if let Ok(type_name) = fork.parse::<Ident>() {
        // Check if we have <...>
        if fork.peek(Token![<]) {
            // Manually consume tokens until we find the matching >
            fork.parse::<Token![<]>()?;

            let mut depth = 1;
            let mut generic_tokens = Vec::new();

            while depth > 0 && !fork.is_empty() {
                if fork.peek(Token![<]) {
                    fork.parse::<Token![<]>()?;
                    generic_tokens.push(TokenTree::Punct(proc_macro2::Punct::new(
                        '<',
                        proc_macro2::Spacing::Alone,
                    )));
                    depth += 1;
                } else if fork.peek(Token![>]) {
                    depth -= 1;
                    if depth > 0 {
                        fork.parse::<Token![>]>()?;
                        generic_tokens.push(TokenTree::Punct(proc_macro2::Punct::new(
                            '>',
                            proc_macro2::Spacing::Alone,
                        )));
                    } else {
                        fork.parse::<Token![>]>()?; // consume the closing >
                    }
                } else {
                    // Parse any token and add it to generic_tokens
                    if let Ok(tt) = fork.parse::<TokenTree>() {
                        generic_tokens.push(tt);
                    } else {
                        break;
                    }
                }
            }

            // Now check for ::method()
            if fork.peek(Token![::]) {
                fork.parse::<Token![::]>()?;
                if let Ok(method) = fork.parse::<Ident>() {
                    if fork.peek(token::Paren) {
                        let args_content;
                        parenthesized!(args_content in fork);

                        if let Ok(args) = args_content.parse_terminated(Expr::parse, Token![,]) {
                            // Successfully parsed! Construct the type with generics
                            let generic_stream: proc_macro2::TokenStream =
                                generic_tokens.into_iter().collect();

                            let func = syn::parse2(quote! {
                                <#type_name<#generic_stream>>::#method
                            })?;

                            let expr = Expr::Call(syn::ExprCall {
                                attrs: vec![],
                                func: Box::new(func),
                                paren_token: syn::token::Paren::default(),
                                args: args.into_iter().collect(),
                            });

                            // Build the type path for the node
                            let type_path = syn::parse2(quote! { #type_name<#generic_stream> })?;
                            let node_type = if let syn::Type::Path(type_path_parsed) = type_path {
                                Some(type_path_parsed.path)
                            } else {
                                None
                            };

                            input.advance_to(&fork);
                            return Ok((expr, node_type));
                        }
                    }
                }
            }
        } else if fork.peek(Token![::]) {
            // No generics, but still Type::method() pattern
            fork.parse::<Token![::]>()?;
            if let Ok(method) = fork.parse::<Ident>() {
                if fork.peek(token::Paren) {
                    let args_content;
                    parenthesized!(args_content in fork);

                    if let Ok(args) = args_content.parse_terminated(Expr::parse, Token![,]) {
                        let func = syn::parse2(quote! {
                            <#type_name>::#method
                        })?;

                        let expr = Expr::Call(syn::ExprCall {
                            attrs: vec![],
                            func: Box::new(func),
                            paren_token: syn::token::Paren::default(),
                            args: args.into_iter().collect(),
                        });

                        // Build simple type path
                        let mut path = syn::Path {
                            leading_colon: None,
                            segments: syn::punctuated::Punctuated::new(),
                        };
                        path.segments.push(syn::PathSegment {
                            ident: type_name,
                            arguments: syn::PathArguments::None,
                        });

                        input.advance_to(&fork);
                        return Ok((expr, Some(path)));
                    }
                }
            }
        }
    }

    // Fall back to regular expression parsing for other cases
    let expr = input.parse::<Expr>()?;
    let node_type = extract_node_type(&expr);
    Ok((expr, node_type))
}

/// Extract the node type from a constructor expression
/// E.g., `PolyBlepOscillator::saw(440.0, 0.6)` -> `PolyBlepOscillator`
/// Also handles `<Type<Generic>>::method` syntax
fn extract_node_type(expr: &Expr) -> Option<syn::Path> {
    match expr {
        Expr::Call(call) => {
            match &*call.func {
                // Regular path like Type::method
                Expr::Path(path_expr) => {
                    // Extract everything except the last segment (the method name)
                    let path = &path_expr.path;
                    if path.segments.len() >= 2 {
                        // Build a new path with all segments except the last
                        let segments: Vec<_> = path
                            .segments
                            .iter()
                            .take(path.segments.len() - 1)
                            .cloned()
                            .collect();
                        let type_path = syn::Path {
                            leading_colon: path.leading_colon,
                            segments: segments.into_iter().collect(),
                        };
                        return Some(type_path);
                    }
                    None
                }
                // Qualified path like <Type>::method or <Type<T>>::method
                _ => {
                    // Try to extract the type from the generated code
                    // The format is <Type>::method, so we need to extract Type
                    // We can do this by converting to string and parsing, but that's fragile
                    // Instead, let's return None and rely on the fact that we can
                    // infer the type from the variable name or context
                    None
                }
            }
        }
        Expr::Path(path_expr) => Some(path_expr.path.clone()),
        _ => None,
    }
}

impl Parse for NodeBlock {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_node_block(input).map(NodeBlock)
    }
}

/// Parse optional leading `[token]` policy prefix on a connection statement.
/// Recognized: `latch`, `linear`, `sinc`, `sinc_iir`.
fn parse_connection_policy(input: ParseStream) -> Result<ConnectionPolicy> {
    if !input.peek(token::Bracket) {
        return Ok(ConnectionPolicy::Default);
    }
    let content;
    bracketed!(content in input);
    let lookahead = content.lookahead1();
    let policy = if lookahead.peek(kw::latch) {
        content.parse::<kw::latch>()?;
        ConnectionPolicy::Latch
    } else if lookahead.peek(kw::linear) {
        content.parse::<kw::linear>()?;
        ConnectionPolicy::Linear
    } else if lookahead.peek(kw::sinc_iir) {
        content.parse::<kw::sinc_iir>()?;
        ConnectionPolicy::SincIir
    } else if lookahead.peek(kw::sinc) {
        content.parse::<kw::sinc>()?;
        ConnectionPolicy::Sinc
    } else {
        return Err(lookahead.error());
    };
    if !content.is_empty() {
        return Err(content.error("unexpected tokens after policy keyword"));
    }
    Ok(policy)
}

/// Parse the body of a connection statement (everything after an
/// optional `connection` keyword): `[<policy>] <source> -> <dest>;`.
/// Shared between `Parse for ConnectionStmt` (which consumes
/// `connection` first), `parse_connection_block` (which uses it inside
/// `connection {}` / `connections {}` block contents), and the block
/// chunker in Task 4.
fn parse_connection_stmt_body(input: ParseStream) -> Result<ConnectionStmt> {
    let policy = parse_connection_policy(input)?;
    let source = parse_connection_expr(input)?;

    // Parse -> as two separate tokens: - and >
    input.parse::<Token![-]>()?;
    input.parse::<Token![>]>()?;

    let dest = parse_connection_expr(input)?;
    input.parse::<Token![;]>()?;

    let span = source
        .span()
        .join(dest.span())
        .unwrap_or_else(|| source.span());

    Ok(ConnectionStmt {
        source,
        dest,
        policy,
        span,
    })
}

// Parse connection block
fn parse_connection_block(input: ParseStream) -> Result<Vec<ConnectionStmt>> {
    // Accept either 'connection' or 'connections'
    if input.peek(kw::connections) {
        input.parse::<kw::connections>()?;
    } else {
        input.parse::<kw::connection>()?;
    }

    let content;
    braced!(content in input);

    let mut connections = Vec::new();
    while !content.is_empty() {
        connections.push(parse_connection_stmt_body(&content)?);
    }

    Ok(connections)
}

impl Parse for ConnectionBlock {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_connection_block(input).map(ConnectionBlock)
    }
}

impl Parse for ConnectionStmt {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::connection>()?;
        parse_connection_stmt_body(input)
    }
}

// Parse connection expressions with operator precedence
fn parse_connection_expr(input: ParseStream) -> Result<ConnectionExpr> {
    parse_additive_expr(input)
}

fn parse_additive_expr(input: ParseStream) -> Result<ConnectionExpr> {
    let mut left = parse_multiplicative_expr(input)?;

    while input.peek(Token![+]) || (input.peek(Token![-]) && !input.peek2(Token![>])) {
        let op = if input.peek(Token![+]) {
            input.parse::<Token![+]>()?;
            BinaryOp::Add
        } else {
            input.parse::<Token![-]>()?;
            BinaryOp::Sub
        };

        let right = parse_multiplicative_expr(input)?;
        left = ConnectionExpr::Binary(Box::new(left), op, Box::new(right));
    }

    Ok(left)
}

fn parse_multiplicative_expr(input: ParseStream) -> Result<ConnectionExpr> {
    let mut left = parse_primary_expr(input)?;

    while input.peek(Token![*]) || input.peek(Token![/]) {
        let op = if input.peek(Token![*]) {
            input.parse::<Token![*]>()?;
            BinaryOp::Mul
        } else {
            input.parse::<Token![/]>()?;
            BinaryOp::Div
        };

        let right = parse_primary_expr(input)?;
        left = ConnectionExpr::Binary(Box::new(left), op, Box::new(right));
    }

    Ok(left)
}

fn parse_primary_expr(input: ParseStream) -> Result<ConnectionExpr> {
    // Handle parenthesized expressions
    if input.peek(token::Paren) {
        let content;
        syn::parenthesized!(content in input);
        return parse_connection_expr(&content);
    }

    // Handle literals
    if input.peek(syn::LitFloat) || input.peek(syn::LitInt) {
        let lit: Expr = input.parse()?;
        return Ok(ConnectionExpr::Literal(lit));
    }

    // Parse identifier or method call
    let ident: Ident = input.parse()?;

    // Check for method call or field access
    let mut expr = ConnectionExpr::Ident(ident.clone());

    loop {
        if input.peek(token::Bracket) {
            // Array indexing
            let content;
            bracketed!(content in input);
            let index: syn::LitInt = content.parse()?;
            let index_val = index.base10_parse::<usize>()?;
            expr = ConnectionExpr::ArrayIndex(Box::new(expr), index_val);
        } else if input.peek(Token![.]) {
            input.parse::<Token![.]>()?;
            let method_name: Ident = input.parse()?;

            // Check if it's a method call (has parens) vs field access
            if input.peek(token::Paren) {
                let content;
                parenthesized!(content in input);
                let args = parse_method_args(&content)?;
                expr = ConnectionExpr::MethodCall(Box::new(expr), method_name, args);
            } else {
                expr = ConnectionExpr::Field(Box::new(expr), method_name);
            }
        } else if input.peek(token::Paren) && matches!(expr, ConnectionExpr::Ident(_)) {
            // Function call
            let content;
            parenthesized!(content in input);
            let args = parse_call_args(&content)?;

            if let ConnectionExpr::Ident(func_name) = expr {
                expr = ConnectionExpr::Call(func_name, args);
            }
        } else {
            break;
        }
    }

    Ok(expr)
}

fn parse_method_args(input: ParseStream) -> Result<Vec<Expr>> {
    let mut args = Vec::new();
    while !input.is_empty() {
        args.push(input.parse()?);
        if !input.peek(Token![,]) {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    Ok(args)
}

fn parse_call_args(input: ParseStream) -> Result<Vec<ConnectionExpr>> {
    let mut args = Vec::new();
    while !input.is_empty() {
        args.push(parse_connection_expr(input)?);
        if !input.peek(Token![,]) {
            break;
        }
        input.parse::<Token![,]>()?;
    }
    Ok(args)
}

/// Helper function to parse EndpointKind from an Ident (for old syntax compatibility)
fn parse_endpoint_kind_from_ident(ident: &Ident) -> Result<EndpointKind> {
    let ident_str = ident.to_string();
    match ident_str.as_str() {
        "stream" => Ok(EndpointKind::Stream),
        "value" => Ok(EndpointKind::Value),
        "event" => Ok(EndpointKind::Event),
        _ => Err(syn::Error::new_spanned(
            ident,
            format!(
                "expected 'stream', 'value', or 'event', found '{}'",
                ident_str
            ),
        )),
    }
}

impl Parse for EndpointKind {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(kw::stream) {
            input.parse::<kw::stream>()?;
            Ok(EndpointKind::Stream)
        } else if lookahead.peek(kw::value) {
            input.parse::<kw::value>()?;
            Ok(EndpointKind::Value)
        } else if lookahead.peek(kw::event) {
            input.parse::<kw::event>()?;
            Ok(EndpointKind::Event)
        } else {
            Err(lookahead.error())
        }
    }
}

impl Parse for ParamSpec {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        bracketed!(content in input);

        let mut range = None;
        let mut curve = None;
        let mut ramp = None;
        let mut center = None;
        let mut step = None;
        let mut unit = None;
        let mut display_name = None;
        let mut smoother = None;

        // Compact syntax: [min..max, center = X, step = Y, unit = " Hz"]
        // First, check if we start with a range (number or negative number)
        if !content.is_empty() {
            let fork = content.fork();
            // Check if first token could be start of a range (not a keyword like `step`)
            let is_range_start = fork.peek(syn::LitFloat)
                || fork.peek(syn::LitInt)
                || fork.peek(Token![-])
                || (fork.peek(Ident) && {
                    let ident: Ident = fork.parse().unwrap();
                    // Not a known keyword
                    !matches!(
                        ident.to_string().as_str(),
                        "step" | "unit" | "name" | "smooth" | "range" | "linear" | "log" | "ramp"
                    )
                });

            if is_range_start {
                // Parse: min..max
                let min = parse_simple_expr(&content)?;
                content.parse::<Token![..]>()?;
                let max = parse_simple_expr(&content)?;
                range = Some(RangeSpec { min, max });

                // Require comma before named options (if any follow)
                if !content.is_empty() {
                    content.parse::<Token![,]>()?;
                }
            }
        }

        // Parse remaining named options (step = X, unit = " Hz", etc.)
        while !content.is_empty() {
            let lookahead = content.lookahead1();

            if lookahead.peek(kw::center) {
                content.parse::<kw::center>()?;
                content.parse::<Token![=]>()?;
                center = Some(parse_simple_expr(&content)?);
            } else if lookahead.peek(kw::step) {
                content.parse::<kw::step>()?;
                content.parse::<Token![=]>()?;
                step = Some(content.parse()?);
            } else if lookahead.peek(kw::unit) {
                content.parse::<kw::unit>()?;
                content.parse::<Token![=]>()?;
                let lit: syn::LitStr = content.parse()?;
                unit = Some(lit.value());
            } else if lookahead.peek(kw::name) {
                content.parse::<kw::name>()?;
                content.parse::<Token![=]>()?;
                let lit: syn::LitStr = content.parse()?;
                display_name = Some(lit.value());
            } else if lookahead.peek(kw::smoother) {
                content.parse::<kw::smoother>()?;
                content.parse::<Token![=]>()?;
                smoother = Some(content.parse()?);
            } else if lookahead.peek(kw::linear) {
                content.parse::<kw::linear>()?;
                curve = Some(Curve::Linear);
            } else if lookahead.peek(kw::log) {
                content.parse::<kw::log>()?;
                curve = Some(Curve::Logarithmic);
            } else if lookahead.peek(kw::ramp) {
                content.parse::<kw::ramp>()?;
                content.parse::<Token![:]>()?;
                let lit: syn::LitInt = content.parse()?;
                ramp = Some(lit.base10_parse()?);
            } else if lookahead.peek(kw::range) {
                // Legacy range(min, max) syntax
                content.parse::<kw::range>()?;
                let range_content;
                parenthesized!(range_content in content);
                let min = range_content.parse()?;
                range_content.parse::<Token![,]>()?;
                let max = range_content.parse()?;
                range = Some(RangeSpec { min, max });
            } else {
                return Err(lookahead.error());
            }

            // Optional trailing comma
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        Ok(ParamSpec {
            range,
            curve,
            ramp,
            center,
            unit,
            smoother,
            step,
            display_name,
            group: None,
        })
    }
}

// Custom keywords
mod kw {
    syn::custom_keyword!(name);
    syn::custom_keyword!(sample_rate);
    syn::custom_keyword!(input);
    syn::custom_keyword!(output);
    syn::custom_keyword!(node);
    syn::custom_keyword!(nodes);
    syn::custom_keyword!(connection);
    syn::custom_keyword!(connections);
    syn::custom_keyword!(stream);
    syn::custom_keyword!(value);
    syn::custom_keyword!(event);
    syn::custom_keyword!(linear);
    syn::custom_keyword!(log);
    syn::custom_keyword!(ramp);
    syn::custom_keyword!(range);
    // NIH-plug related keywords
    syn::custom_keyword!(nih_params);
    syn::custom_keyword!(center);
    syn::custom_keyword!(unit);
    syn::custom_keyword!(smoother);
    syn::custom_keyword!(step);
    syn::custom_keyword!(group);
    // Connection policy keywords (Phase 2 / Task 2.3).
    // Note: `linear` already exists above (used for curve specs); reuse it here.
    syn::custom_keyword!(latch);
    syn::custom_keyword!(sinc);
    syn::custom_keyword!(sinc_iir);
}
