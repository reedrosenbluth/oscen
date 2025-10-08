use super::ast::*;
use syn::{
    braced, bracketed, parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token, Expr, Ident, Result, Token,
};

impl Parse for GraphDef {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut name = None;
        let mut items = Vec::new();

        // Check for optional name declaration at the start
        if input.peek(kw::name) {
            input.parse::<kw::name>()?;
            input.parse::<Token![:]>()?;
            name = Some(input.parse()?);
            input.parse::<Token![;]>()?;
        }

        while !input.is_empty() {
            items.push(input.parse()?);
        }

        Ok(GraphDef { name, items })
    }
}

impl Parse for GraphItem {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();

        if lookahead.peek(kw::input) {
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

impl Parse for InputDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::input>()?;
        let kind = input.parse()?;
        let name = input.parse()?;

        let mut default = None;
        let mut spec = None;

        // Parse optional default value
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            // Parse a literal or simple expression, but stop at `[` or `;`
            // We can't use parse::<Expr>() because it will consume the `[...]` as array indexing
            default = Some(parse_simple_expr(input)?);

            // Parse optional parameter spec in brackets
            if input.peek(token::Bracket) {
                spec = Some(input.parse()?);
            }
        }

        input.parse::<Token![;]>()?;

        Ok(InputDecl {
            kind,
            name,
            default,
            spec,
        })
    }
}

// Parse a simple expression that won't consume brackets
fn parse_simple_expr(input: ParseStream) -> Result<Expr> {
    // Parse literals (numbers, strings, etc.) or simple paths
    // Stop when we see `[` or `;`
    // DON'T use parse::<Expr>() because it will consume brackets as array indexing!
    if input.peek(syn::LitFloat) || input.peek(syn::LitInt) || input.peek(syn::LitStr) || input.peek(syn::LitBool) {
        let lit: syn::Lit = input.parse()?;
        Ok(Expr::Lit(syn::ExprLit {
            attrs: vec![],
            lit,
        }))
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
            expr: Box::new(Expr::Lit(syn::ExprLit {
                attrs: vec![],
                lit,
            })),
        }))
    } else {
        Err(input.error("expected literal or identifier for default value"))
    }
}

impl Parse for OutputDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::output>()?;
        let kind = input.parse()?;
        let name = input.parse()?;
        input.parse::<Token![;]>()?;

        Ok(OutputDecl { kind, name })
    }
}

impl Parse for NodeDecl {
    fn parse(input: ParseStream) -> Result<Self> {
        input.parse::<kw::node>()?;
        let name = input.parse()?;
        input.parse::<Token![=]>()?;
        let constructor: Expr = input.parse()?;
        input.parse::<Token![;]>()?;

        let node_type = extract_node_type(&constructor);
        Ok(NodeDecl { name, constructor, node_type })
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
        let name = content.parse()?;
        content.parse::<Token![=]>()?;
        let constructor: Expr = content.parse()?;
        content.parse::<Token![;]>()?;

        let node_type = extract_node_type(&constructor);
        nodes.push(NodeDecl { name, constructor, node_type });
    }

    Ok(nodes)
}

/// Extract the node type from a constructor expression
/// E.g., `PolyBlepOscillator::saw(440.0, 0.6)` -> `PolyBlepOscillator`
fn extract_node_type(expr: &Expr) -> Option<syn::Path> {
    match expr {
        Expr::Call(call) => {
            // Check if the function is a path like Type::method
            if let Expr::Path(path_expr) = &*call.func {
                // Extract everything except the last segment (the method name)
                let path = &path_expr.path;
                if path.segments.len() >= 2 {
                    // Clone all segments except the last one (which is the method name)
                    let mut type_path = path.clone();
                    type_path.segments.pop();
                    return Some(type_path);
                }
            }
            None
        }
        _ => None,
    }
}

impl Parse for NodeBlock {
    fn parse(input: ParseStream) -> Result<Self> {
        parse_node_block(input).map(NodeBlock)
    }
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
        let source = parse_connection_expr(&content)?;

        // Parse -> as two separate tokens: - and >
        content.parse::<Token![-]>()?;
        content.parse::<Token![>]>()?;

        let dest = parse_connection_expr(&content)?;
        content.parse::<Token![;]>()?;

        connections.push(ConnectionStmt { source, dest });
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
        let source = parse_connection_expr(input)?;

        // Parse -> as two separate tokens: - and >
        input.parse::<Token![-]>()?;
        input.parse::<Token![>]>()?;

        let dest = parse_connection_expr(input)?;
        input.parse::<Token![;]>()?;

        Ok(ConnectionStmt { source, dest })
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
        if input.peek(Token![.]) {
            input.parse::<Token![.]>()?;
            let method_name: Ident = input.parse()?;

            // Check if it's a method call
            if input.peek(token::Paren) {
                let content;
                parenthesized!(content in input);
                let args = parse_method_args(&content)?;
                expr = ConnectionExpr::Method(Box::new(expr), method_name, args);
            } else {
                // Field access (treat as method with no parens)
                expr = ConnectionExpr::Method(Box::new(expr), method_name, vec![]);
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

        // Parse comma-separated list using Punctuated
        let items = content.parse_terminated(ParamSpecItem::parse, Token![,])?;

        for item in items {
            match item {
                ParamSpecItem::Range(min, max) => {
                    range = Some(RangeSpec { min, max });
                }
                ParamSpecItem::Linear => {
                    curve = Some(Curve::Linear);
                }
                ParamSpecItem::Log => {
                    curve = Some(Curve::Logarithmic);
                }
                ParamSpecItem::Ramp(samples) => {
                    ramp = Some(samples);
                }
            }
        }

        Ok(ParamSpec { range, curve, ramp })
    }
}

// Individual parameter spec items
enum ParamSpecItem {
    Range(syn::Expr, syn::Expr),
    Linear,
    Log,
    Ramp(usize),
}

impl Parse for ParamSpecItem {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();

        if lookahead.peek(kw::range) {
            input.parse::<kw::range>()?;
            let content;
            parenthesized!(content in input);
            let min = content.parse()?;
            content.parse::<Token![,]>()?;
            let max = content.parse()?;
            Ok(ParamSpecItem::Range(min, max))
        } else if lookahead.peek(kw::linear) {
            input.parse::<kw::linear>()?;
            Ok(ParamSpecItem::Linear)
        } else if lookahead.peek(kw::log) {
            input.parse::<kw::log>()?;
            Ok(ParamSpecItem::Log)
        } else if lookahead.peek(kw::ramp) {
            input.parse::<kw::ramp>()?;
            let content;
            parenthesized!(content in input);
            let lit: syn::LitInt = content.parse()?;
            Ok(ParamSpecItem::Ramp(lit.base10_parse()?))
        } else {
            Err(lookahead.error())
        }
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
}
