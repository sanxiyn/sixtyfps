/* LICENSE BEGIN
    This file is part of the SixtyFPS Project -- https://sixtyfps.io
    Copyright (c) 2020 Olivier Goffart <olivier.goffart@sixtyfps.io>
    Copyright (c) 2020 Simon Hausmann <simon.hausmann@sixtyfps.io>

    SPDX-License-Identifier: GPL-3.0-only
    This file is also available under commercial licensing terms.
    Please contact info@sixtyfps.io for more information.
LICENSE END */
//! Passes that resolve the property binding expression.
//!
//! Before this pass, all the expression are of type Expression::Uncompiled,
//! and there should no longer be Uncompiled expression after this pass.
//!
//! Most of the code for the resolving actualy lies in the expression_tree module

use crate::diagnostics::{BuildDiagnostics, SpannedWithSourceFile};
use crate::expression_tree::*;
use crate::langtype::Type;
use crate::object_tree::*;
use crate::parser::{
    identifier_text, syntax_nodes, NodeOrTokenWithSourceFile, SyntaxKind, SyntaxNodeWithSourceFile,
};
use crate::typeregister::TypeRegister;
use std::{collections::HashMap, rc::Rc};

/// This represeresent a scope for the Component, where Component is the repeated component, but
/// does not represent a component in the .60 file
#[derive(Clone)]
struct ComponentScope(Vec<ElementRc>);

fn resolve_expression(
    expr: &mut Expression,
    property_name: Option<&str>,
    property_type: Type,
    scope: &ComponentScope,
    type_register: &TypeRegister,
    diag: &mut BuildDiagnostics,
) {
    if let Expression::Uncompiled(node) = expr {
        let mut lookup_ctx = LookupCtx {
            property_name,
            property_type,
            component_scope: &scope.0,
            diag,
            arguments: vec![],
            type_register,
        };

        let new_expr = match node.kind() {
            SyntaxKind::CallbackConnection => {
                //FIXME: proper callback suport (node is a codeblock)
                Expression::from_callback_connection(node.clone().into(), &mut lookup_ctx)
            }
            SyntaxKind::Expression => {
                //FIXME again: this happen for non-binding expression (i.e: model)
                Expression::from_expression_node(node.clone().into(), &mut lookup_ctx)
                    .maybe_convert_to(lookup_ctx.property_type.clone(), node, diag)
            }
            SyntaxKind::BindingExpression => {
                Expression::from_binding_expression_node(node.clone(), &mut lookup_ctx)
            }
            SyntaxKind::TwoWayBinding => {
                Expression::from_two_way_binding(node.clone().into(), &mut lookup_ctx)
            }
            _ => {
                debug_assert!(diag.has_error());
                Expression::Invalid
            }
        };
        *expr = new_expr;
    }
}

pub fn resolve_expressions(doc: &Document, diag: &mut BuildDiagnostics) {
    for component in doc.inner_components.iter() {
        let scope = ComponentScope(vec![component.root_element.clone()]);

        recurse_elem(&component.root_element, &scope, &mut |elem, scope| {
            let mut new_scope = scope.clone();
            let mut is_repeated = elem.borrow().repeated.is_some();
            if is_repeated {
                new_scope.0.push(elem.clone())
            }
            new_scope.0.push(elem.clone());
            visit_element_expressions(elem, |expr, property_name, property_type| {
                if is_repeated {
                    // The first expression is always the model and it needs to be resolved with the parent scope
                    debug_assert!(elem.borrow().repeated.as_ref().is_none()); // should be none because it is taken by the visit_element_expressions function
                    resolve_expression(
                        expr,
                        property_name,
                        property_type(),
                        scope,
                        &doc.local_registry,
                        diag,
                    );
                    is_repeated = false;
                } else {
                    resolve_expression(
                        expr,
                        property_name,
                        property_type(),
                        &new_scope,
                        &doc.local_registry,
                        diag,
                    )
                }
            });
            new_scope.0.pop();
            new_scope
        })
    }
}

/// Contains information which allow to lookup identifier in expressions
pub struct LookupCtx<'a> {
    /// the name of the property for which this expression refers.
    property_name: Option<&'a str>,

    /// the type of the property for which this expression refers.
    /// (some property come in the scope)
    property_type: Type,

    /// Here is the stack in which id applies
    component_scope: &'a [ElementRc],

    /// Somewhere to report diagnostics
    diag: &'a mut BuildDiagnostics,

    /// The name of the arguments of the callback or function
    arguments: Vec<String>,

    /// The type register in which to look for Globals
    type_register: &'a TypeRegister,
}

impl<'a> LookupCtx<'a> {
    /// Return a context that is just suitable to build simple const expression
    pub fn empty_context(type_register: &'a TypeRegister, diag: &'a mut BuildDiagnostics) -> Self {
        Self {
            property_name: Default::default(),
            property_type: Default::default(),
            component_scope: Default::default(),
            diag,
            arguments: Default::default(),
            type_register,
        }
    }
}

fn find_element_by_id(roots: &[ElementRc], name: &str) -> Option<ElementRc> {
    for e in roots.iter().rev() {
        if e.borrow().id == name {
            return Some(e.clone());
        }
        for x in &e.borrow().children {
            if x.borrow().repeated.is_some() {
                continue;
            }
            if let Some(x) = find_element_by_id(&[x.clone()], name) {
                return Some(x);
            }
        }
    }
    None
}

/// Find the parent element to a given element.
/// (since there is no parent mapping we need to fo an exhaustive search)
fn find_parent_element(e: &ElementRc) -> Option<ElementRc> {
    fn recurse(base: &ElementRc, e: &ElementRc) -> Option<ElementRc> {
        for child in &base.borrow().children {
            if Rc::ptr_eq(child, e) {
                return Some(base.clone());
            }
            if let Some(x) = recurse(child, e) {
                return Some(x);
            }
        }
        None
    }

    let root = e.borrow().enclosing_component.upgrade().unwrap().root_element.clone();
    if Rc::ptr_eq(&root, e) {
        return None;
    }
    recurse(&root, e)
}

/// If the type of the expression is a percentage, and the current property evaluated is
/// `width` or `height`, attempt to multiply by the parent `width` or `height`
fn attempt_percent_conversion(
    ctx: &mut LookupCtx,
    e: Expression,
    node: &dyn SpannedWithSourceFile,
) -> Expression {
    if ctx.property_type != Type::Length || e.ty() != Type::Percent {
        return e;
    }

    const RELATIVE_TO_PARENT_PROPERTIES: [&str; 2] = ["width", "height"];
    let property_name = ctx.property_name.unwrap_or_default();
    if !RELATIVE_TO_PARENT_PROPERTIES.contains(&property_name) {
        ctx.diag.push_error(
            format!(
                "Automatic conversion from percentage to lenght is only possible for the properties {}",
                RELATIVE_TO_PARENT_PROPERTIES.join(" and ")
            ),
            node
        );
        return Expression::Invalid;
    }

    let mut parent = ctx.component_scope.last().and_then(find_parent_element);
    while let Some(p) = parent {
        let ty = p.borrow().lookup_property(property_name);
        if ty == Type::Length {
            return Expression::BinaryExpression {
                lhs: Box::new(Expression::BinaryExpression {
                    lhs: Box::new(e),
                    rhs: Box::new(Expression::NumberLiteral(0.01, Unit::None)),
                    op: '*',
                }),
                rhs: Box::new(Expression::PropertyReference(NamedReference {
                    element: Rc::downgrade(&p),
                    name: property_name.to_string(),
                })),
                op: '*',
            };
        }
        parent = find_parent_element(&p);
    }

    ctx.diag.push_error("Cannot find parent property to apply relative lenght".into(), node);
    Expression::Invalid
}

impl Expression {
    pub fn from_binding_expression_node(
        node: SyntaxNodeWithSourceFile,
        ctx: &mut LookupCtx,
    ) -> Self {
        debug_assert_eq!(node.kind(), SyntaxKind::BindingExpression);
        let e = node
            .child_node(SyntaxKind::Expression)
            .map(|n| Self::from_expression_node(n.into(), ctx))
            .or_else(|| {
                node.child_node(SyntaxKind::CodeBlock)
                    .map(|c| Self::from_codeblock_node(c.into(), ctx))
            })
            .unwrap_or(Self::Invalid);
        let e = attempt_percent_conversion(ctx, e, &node);
        e.maybe_convert_to(ctx.property_type.clone(), &node, &mut ctx.diag)
    }

    fn from_codeblock_node(node: syntax_nodes::CodeBlock, ctx: &mut LookupCtx) -> Expression {
        debug_assert_eq!(node.kind(), SyntaxKind::CodeBlock);
        Expression::CodeBlock(
            node.children()
                .filter(|n| n.kind() == SyntaxKind::Expression)
                .map(|n| Self::from_expression_node(n.into(), ctx))
                .collect(),
        )
    }

    fn from_callback_connection(
        node: syntax_nodes::CallbackConnection,
        ctx: &mut LookupCtx,
    ) -> Expression {
        ctx.arguments =
            node.DeclaredIdentifier().map(|x| identifier_text(&x).unwrap_or_default()).collect();
        Self::from_codeblock_node(node.CodeBlock(), ctx)
    }

    fn from_two_way_binding(node: syntax_nodes::TwoWayBinding, ctx: &mut LookupCtx) -> Expression {
        let e = Self::from_expression_node(node.Expression(), ctx);
        let ty = e.ty();
        match e {
            Expression::PropertyReference(n) => {
                if ty != ctx.property_type {
                    ctx.diag.push_error(
                        "The property does not have the same type as the bound property".into(),
                        &node,
                    );
                }
                Expression::TwoWayBinding(n, None)
            }
            _ => {
                ctx.diag.push_error(
                    "The expression in a two way binding must be a property reference".into(),
                    &node,
                );
                e
            }
        }
    }

    fn from_expression_node(node: syntax_nodes::Expression, ctx: &mut LookupCtx) -> Self {
        node.Expression()
            .map(|n| Self::from_expression_node(n, ctx))
            .or_else(|| {
                node.BangExpression().map(|n| Self::from_bang_expression_node(n.into(), ctx))
            })
            .or_else(|| node.QualifiedName().map(|s| Self::from_qualified_name_node(s.into(), ctx)))
            .or_else(|| {
                node.child_text(SyntaxKind::StringLiteral).map(|s| {
                    unescape_string(&s).map(Self::StringLiteral).unwrap_or_else(|| {
                        ctx.diag.push_error("Cannot parse string literal".into(), &node);
                        Self::Invalid
                    })
                })
            })
            .or_else(|| {
                node.child_text(SyntaxKind::NumberLiteral)
                    .map(parse_number_literal)
                    .transpose()
                    .unwrap_or_else(|e| {
                        ctx.diag.push_error(e, &node);
                        Some(Self::Invalid)
                    })
            })
            .or_else(|| {
                node.child_text(SyntaxKind::ColorLiteral).map(|s| {
                    parse_color_literal(&s)
                        .map(|i| Expression::Cast {
                            from: Box::new(Expression::NumberLiteral(i as _, Unit::None)),
                            to: Type::Color,
                        })
                        .unwrap_or_else(|| {
                            ctx.diag.push_error("Invalid color literal".into(), &node);
                            Self::Invalid
                        })
                })
            })
            .or_else(|| {
                node.FunctionCallExpression().map(|n| Self::from_function_call_node(n, ctx))
            })
            .or_else(|| node.SelfAssignment().map(|n| Self::from_self_assignement_node(n, ctx)))
            .or_else(|| node.BinaryExpression().map(|n| Self::from_binary_expression_node(n, ctx)))
            .or_else(|| {
                node.UnaryOpExpression().map(|n| Self::from_unaryop_expression_node(n, ctx))
            })
            .or_else(|| {
                node.ConditionalExpression().map(|n| Self::from_conditional_expression_node(n, ctx))
            })
            .or_else(|| node.ObjectLiteral().map(|n| Self::from_object_literal_node(n, ctx)))
            .or_else(|| node.Array().map(|n| Self::from_array_node(n, ctx)))
            .or_else(|| node.CodeBlock().map(|n| Self::from_codeblock_node(n, ctx)))
            .unwrap_or(Self::Invalid)
    }

    fn from_bang_expression_node(node: SyntaxNodeWithSourceFile, ctx: &mut LookupCtx) -> Self {
        match identifier_text(&node).as_deref() {
            None => {
                debug_assert!(false, "the parser should not allow that");
                ctx.diag.push_error("Missing bang keyword".into(), &node);
                Self::Invalid
            }
            Some("img") => {
                // FIXME: we probably need a better syntax and make this at another level.
                let s = match node
                    .child_node(SyntaxKind::Expression)
                    .map_or(Self::Invalid, |n| Self::from_expression_node(n.into(), ctx))
                {
                    Expression::StringLiteral(p) => p,
                    _ => {
                        ctx.diag.push_error("img! Must be followed by a valid path".into(), &node);
                        return Self::Invalid;
                    }
                };

                let absolute_source_path = {
                    let path = std::path::Path::new(&s);

                    if path.is_absolute() || s.starts_with("http://") || s.starts_with("https://") {
                        s
                    } else {
                        let path = node
                            .source_file
                            .unwrap_or_default()
                            .parent()
                            .map(|b| b.join(path))
                            .unwrap_or_else(|| path.to_owned());
                        if path.is_absolute() {
                            path.to_string_lossy().to_string()
                        } else {
                            std::env::current_dir()
                                .map(|b| b.join(&path))
                                .unwrap_or(path)
                                .to_string_lossy()
                                .to_string()
                        }
                    }
                };

                Expression::ResourceReference(ResourceReference::AbsolutePath(absolute_source_path))
            }
            Some(x) => {
                ctx.diag.push_error(format!("Unknown bang keyword `{}`", x), &node);
                Self::Invalid
            }
        }
    }

    /// Perform the lookup
    fn from_qualified_name_node(node: SyntaxNodeWithSourceFile, ctx: &mut LookupCtx) -> Self {
        debug_assert_eq!(node.kind(), SyntaxKind::QualifiedName);

        let mut it = node
            .children_with_tokens()
            .filter(|n| n.kind() == SyntaxKind::Identifier)
            .filter_map(|n| n.into_token());

        let first = if let Some(first) = it.next() {
            first
        } else {
            // There must be at least one member (parser should ensure that)
            debug_assert!(ctx.diag.has_error());
            return Self::Invalid;
        };

        let first_str = crate::parser::normalize_identifier(first.text().as_str());

        if let Some(index) = ctx.arguments.iter().position(|x| x == &first_str) {
            let ty = match &ctx.property_type {
                Type::Callback { args, .. } | Type::Function { args, .. } => args[index].clone(),
                _ => panic!("There should only be argument within functions or callback"),
            };
            let e = Expression::FunctionParameterReference { index, ty };
            return maybe_lookup_object(e, it, ctx);
        }

        let elem_opt = match first_str.as_str() {
            "self" => ctx.component_scope.last().cloned(),
            "parent" => ctx.component_scope.last().and_then(find_parent_element),
            "true" => return Self::BoolLiteral(true),
            "false" => return Self::BoolLiteral(false),
            _ => find_element_by_id(ctx.component_scope, &first_str).or_else(|| {
                if let Type::Component(c) = ctx.type_register.lookup(&first_str) {
                    if c.is_global() {
                        return Some(c.root_element.clone());
                    }
                }
                None
            }),
        };

        if let Some(elem) = elem_opt {
            return continue_lookup_within_element(&elem, &mut it, node, ctx);
        }

        for elem in ctx.component_scope.iter().rev() {
            if let Some(repeated) = &elem.borrow().repeated {
                if first_str == repeated.index_id {
                    return Expression::RepeaterIndexReference { element: Rc::downgrade(elem) };
                } else if first_str == repeated.model_data_id {
                    let base = Expression::RepeaterModelReference { element: Rc::downgrade(elem) };
                    return maybe_lookup_object(base, it, ctx);
                }
            }

            let property = elem.borrow().lookup_property(&first_str);
            if property.is_property_type() {
                let prop = Self::PropertyReference(NamedReference {
                    element: Rc::downgrade(&elem),
                    name: first_str,
                });
                return maybe_lookup_object(prop, it, ctx);
            } else if matches!(property, Type::Callback{..}) {
                if let Some(x) = it.next() {
                    ctx.diag.push_error("Cannot access fields of callback".into(), &x)
                }
                return Self::CallbackReference(NamedReference {
                    element: Rc::downgrade(&elem),
                    name: first_str,
                });
            } else if property.is_object_type() {
                todo!("Continue lookling up");
            }
        }

        if it.next().is_some() {
            ctx.diag.push_error(format!("Cannot access id '{}'", first_str), &node);
            return Expression::Invalid;
        }

        match &ctx.property_type {
            Type::Color => {
                if let Some(c) = css_color_parser2::NAMED_COLORS.get(first_str.as_str()) {
                    let value = ((c.a as u32 * 255) << 24)
                        | ((c.r as u32) << 16)
                        | ((c.g as u32) << 8)
                        | (c.b as u32);
                    return Expression::Cast {
                        from: Box::new(Expression::NumberLiteral(value as f64, Unit::None)),
                        to: Type::Color,
                    };
                }
            }
            Type::Easing => {
                // These value are coming from CSSn with - replaced by _
                let value = match first_str.as_str() {
                    "linear" => Some(EasingCurve::Linear),
                    "ease" => Some(EasingCurve::CubicBezier(0.25, 0.1, 0.25, 1.0)),
                    "ease_in" => Some(EasingCurve::CubicBezier(0.42, 0.0, 1.0, 1.0)),
                    "ease_in_out" => Some(EasingCurve::CubicBezier(0.42, 0.0, 0.58, 1.0)),
                    "ease_out" => Some(EasingCurve::CubicBezier(0.0, 0.0, 0.58, 1.0)),
                    "cubic_bezier" => {
                        return Expression::BuiltinMacroReference(
                            BuiltinMacroFunction::CubicBezier,
                            first.into(),
                        )
                    }
                    _ => None,
                };
                if let Some(curve) = value {
                    return Expression::EasingCurve(curve);
                }
            }
            Type::Enumeration(enumeration) => {
                if let Some(value) = enumeration.clone().try_value_from_string(&first_str) {
                    return Expression::EnumerationValue(value);
                }
            }
            _ => {}
        }

        // Builtin functions  FIXME: handle that in a registery or something
        match first_str.as_str() {
            "debug" => return Expression::BuiltinFunctionReference(BuiltinFunction::Debug),
            "mod" => return Expression::BuiltinFunctionReference(BuiltinFunction::Mod),
            "round" => return Expression::BuiltinFunctionReference(BuiltinFunction::Round),
            "ceil" => return Expression::BuiltinFunctionReference(BuiltinFunction::Ceil),
            "floor" => return Expression::BuiltinFunctionReference(BuiltinFunction::Floor),
            "rgb" => return Expression::BuiltinFunctionReference(BuiltinFunction::Rgb),
            "max" => {
                return Expression::BuiltinMacroReference(BuiltinMacroFunction::Max, first.into())
            }
            "min" => {
                return Expression::BuiltinMacroReference(BuiltinMacroFunction::Min, first.into())
            }
            _ => {}
        };

        // Attempt to recover if the user wanted to write "-"
        if let Some(minus_pos) = first.text().find('-') {
            let report_minus_error = |ctx: &mut LookupCtx| {
                ctx.diag.push_error(format!("Unknown unqualified identifier '{}'. Use space before the '-' if you meant a substraction.", first.text()), &node);
            };
            let first_str = &first.text()[0..minus_pos];
            for elem in ctx.component_scope.iter().rev() {
                if let Some(repeated) = &elem.borrow().repeated {
                    if first_str == repeated.index_id || first_str == repeated.model_data_id {
                        report_minus_error(ctx);
                        return Expression::Invalid;
                    }
                }

                let property = elem.borrow().lookup_property(&first_str);
                if property.is_property_type() {
                    report_minus_error(ctx);
                    return Expression::Invalid;
                }
            }
        }

        ctx.diag.push_error(format!("Unknown unqualified identifier '{}'", first.text()), &node);

        Self::Invalid
    }

    fn from_function_call_node(
        node: syntax_nodes::FunctionCallExpression,
        ctx: &mut LookupCtx,
    ) -> Expression {
        let mut sub_expr = node.Expression().map(|n| {
            (Self::from_expression_node(n.clone(), ctx), NodeOrTokenWithSourceFile::from(n.0))
        });

        let mut arguments = Vec::new();

        let (function, f_node) =
            sub_expr.next().unwrap_or_else(|| (Expression::Invalid, node.0.clone().into()));

        let function = match function {
            Expression::BuiltinMacroReference(mac, n) => match mac {
                BuiltinMacroFunction::Min => {
                    return min_max_macro(n, '<', sub_expr.collect(), &mut ctx.diag);
                }
                BuiltinMacroFunction::Max => {
                    return min_max_macro(n, '>', sub_expr.collect(), &mut ctx.diag);
                }
                BuiltinMacroFunction::CubicBezier => {
                    let mut has_error = None;
                    // FIXME: this is not pretty to be handling there.
                    // Maybe "cubic_bezier" should be a function that is lowered later
                    let mut a = || match sub_expr.next() {
                        None => {
                            has_error.get_or_insert((f_node.clone(), "Not enough arguments"));
                            0.
                        }
                        Some((Expression::NumberLiteral(val, Unit::None), _)) => val as f32,
                        Some((_, n)) => {
                            has_error.get_or_insert((
                                n,
                                "Arguments to cubic bezier curve must be number literal",
                            ));
                            0.
                        }
                    };
                    let expr =
                        Expression::EasingCurve(EasingCurve::CubicBezier(a(), a(), a(), a()));
                    if let Some((_, n)) = sub_expr.next() {
                        has_error.get_or_insert((n, "Too many argument for bezier curve"));
                    }
                    if let Some((n, msg)) = has_error {
                        ctx.diag.push_error(msg.into(), &n);
                    }

                    return expr;
                }
            },
            Expression::MemberFunction { base, base_node, member } => {
                arguments.push((*base, base_node));
                member
            }
            _ => Box::new(function),
        };
        arguments.extend(sub_expr);

        let arguments = match function.ty() {
            Type::Function { args, .. } | Type::Callback { args, .. } => {
                if arguments.len() != args.len() {
                    ctx.diag.push_error(
                        format!(
                            "The callback or function expects {} arguments, but {} are provided",
                            args.len(),
                            arguments.len()
                        ),
                        &node,
                    );
                    arguments.into_iter().map(|x| x.0).collect()
                } else {
                    arguments
                        .into_iter()
                        .zip(args.iter())
                        .map(|((e, node), ty)| e.maybe_convert_to(ty.clone(), &node, &mut ctx.diag))
                        .collect()
                }
            }
            _ => {
                ctx.diag.push_error("The expression is not a function".into(), &node);
                arguments.into_iter().map(|x| x.0).collect()
            }
        };

        Expression::FunctionCall { function, arguments }
    }

    fn from_self_assignement_node(
        node: syntax_nodes::SelfAssignment,
        ctx: &mut LookupCtx,
    ) -> Expression {
        let (lhs_n, rhs_n) = node.Expression();
        let lhs = Self::from_expression_node(lhs_n, ctx);
        let op = None
            .or(node.child_token(SyntaxKind::PlusEqual).and(Some('+')))
            .or(node.child_token(SyntaxKind::MinusEqual).and(Some('-')))
            .or(node.child_token(SyntaxKind::StarEqual).and(Some('*')))
            .or(node.child_token(SyntaxKind::DivEqual).and(Some('/')))
            .or(node.child_token(SyntaxKind::Equal).and(Some('=')))
            .unwrap_or('_');
        if !lhs.is_rw() && lhs.ty() != Type::Invalid {
            ctx.diag.push_error(
                format!(
                    "{} need to be done on a property",
                    if op == '=' { "Assignement" } else { "Self assignement" }
                ),
                &node,
            );
        }
        let rhs = Self::from_expression_node(rhs_n.clone(), ctx).maybe_convert_to(
            lhs.ty(),
            &rhs_n,
            &mut ctx.diag,
        );
        Expression::SelfAssignment { lhs: Box::new(lhs), rhs: Box::new(rhs), op }
    }

    fn from_binary_expression_node(
        node: syntax_nodes::BinaryExpression,
        ctx: &mut LookupCtx,
    ) -> Expression {
        let op = None
            .or(node.child_token(SyntaxKind::Plus).and(Some('+')))
            .or(node.child_token(SyntaxKind::Minus).and(Some('-')))
            .or(node.child_token(SyntaxKind::Star).and(Some('*')))
            .or(node.child_token(SyntaxKind::Div).and(Some('/')))
            .or(node.child_token(SyntaxKind::LessEqual).and(Some('≤')))
            .or(node.child_token(SyntaxKind::GreaterEqual).and(Some('≥')))
            .or(node.child_token(SyntaxKind::LAngle).and(Some('<')))
            .or(node.child_token(SyntaxKind::RAngle).and(Some('>')))
            .or(node.child_token(SyntaxKind::EqualEqual).and(Some('=')))
            .or(node.child_token(SyntaxKind::NotEqual).and(Some('!')))
            .or(node.child_token(SyntaxKind::AndAnd).and(Some('&')))
            .or(node.child_token(SyntaxKind::OrOr).and(Some('|')))
            .unwrap_or('_');

        let (lhs_n, rhs_n) = node.Expression();
        let lhs = Self::from_expression_node(lhs_n.clone(), ctx);
        let rhs = Self::from_expression_node(rhs_n.clone(), ctx);

        let expected_ty = match operator_class(op) {
            OperatorClass::ComparisonOp => {
                Self::common_target_type_for_type_list([lhs.ty(), rhs.ty()].iter().cloned())
            }
            OperatorClass::LogicalOp => Type::Bool,
            OperatorClass::ArithmeticOp => {
                macro_rules! unit_operations {
                    ($($unit:ident)*) => {
                        match (op, lhs.ty(), rhs.ty()) {
                            ('+', Type::String, _) => Type::String,
                            ('+', _, Type::String) => Type::String,

                            $(
                                ('+', Type::$unit, _) => Type::$unit,
                                ('-', Type::$unit, _) => Type::$unit,
                                ('*', Type::$unit, _) => {
                                    return Expression::BinaryExpression {
                                        lhs: Box::new(lhs),
                                        rhs: Box::new(rhs.maybe_convert_to(
                                            Type::Float32,
                                            &lhs_n,
                                            &mut ctx.diag,
                                        )),
                                        op,
                                    }
                                }
                                ('*', _, Type::$unit) => {
                                    return Expression::BinaryExpression {
                                        lhs: Box::new(lhs.maybe_convert_to(
                                            Type::Float32,
                                            &lhs_n,
                                            &mut ctx.diag,
                                        )),
                                        rhs: Box::new(rhs),
                                        op,
                                    }
                                }
                                ('/', Type::$unit, Type::$unit) => {
                                    return Expression::BinaryExpression {
                                        lhs: Box::new(lhs),
                                        rhs: Box::new(rhs),
                                        op,
                                    }
                                }
                                ('/', Type::$unit, _) => {
                                    return Expression::BinaryExpression {
                                        lhs: Box::new(lhs),
                                        rhs: Box::new(rhs.maybe_convert_to(
                                            Type::Float32,
                                            &lhs_n,
                                            &mut ctx.diag,
                                        )),
                                        op,
                                    }
                                }
                            )*
                            _ => Type::Float32,
                        }
                    };
                }
                unit_operations!(Duration Length LogicalLength)
            }
        };
        Expression::BinaryExpression {
            lhs: Box::new(lhs.maybe_convert_to(expected_ty.clone(), &lhs_n, &mut ctx.diag)),
            rhs: Box::new(rhs.maybe_convert_to(expected_ty, &rhs_n, &mut ctx.diag)),
            op,
        }
    }

    fn from_unaryop_expression_node(
        node: syntax_nodes::UnaryOpExpression,
        ctx: &mut LookupCtx,
    ) -> Expression {
        let exp_n = node.Expression();
        let exp = Self::from_expression_node(exp_n, ctx);

        Expression::UnaryOp {
            sub: Box::new(exp),
            op: None
                .or(node.child_token(SyntaxKind::Plus).and(Some('+')))
                .or(node.child_token(SyntaxKind::Minus).and(Some('-')))
                .or(node.child_token(SyntaxKind::Bang).and(Some('!')))
                .unwrap_or('_'),
        }
    }

    fn from_conditional_expression_node(
        node: syntax_nodes::ConditionalExpression,
        ctx: &mut LookupCtx,
    ) -> Expression {
        let (condition_n, true_expr_n, false_expr_n) = node.Expression();
        // FIXME: we should we add bool to the context
        let condition = Self::from_expression_node(condition_n.clone(), ctx).maybe_convert_to(
            Type::Bool,
            &condition_n,
            &mut ctx.diag,
        );
        let true_expr = Self::from_expression_node(true_expr_n.clone(), ctx);
        let false_expr = Self::from_expression_node(false_expr_n.clone(), ctx);
        let result_ty = Self::common_target_type_for_type_list(
            [true_expr.ty(), false_expr.ty()].iter().cloned(),
        );
        let true_expr = true_expr.maybe_convert_to(result_ty.clone(), &true_expr_n, &mut ctx.diag);
        let false_expr = false_expr.maybe_convert_to(result_ty, &false_expr_n, &mut ctx.diag);
        Expression::Condition {
            condition: Box::new(condition),
            true_expr: Box::new(true_expr),
            false_expr: Box::new(false_expr),
        }
    }

    fn from_object_literal_node(
        node: syntax_nodes::ObjectLiteral,
        ctx: &mut LookupCtx,
    ) -> Expression {
        let values: HashMap<String, Expression> = node
            .ObjectMember()
            .map(|n| {
                (
                    identifier_text(&n).unwrap_or_default(),
                    Expression::from_expression_node(n.Expression(), ctx),
                )
            })
            .collect();
        let ty = Type::Object {
            fields: values.iter().map(|(k, v)| (k.clone(), v.ty())).collect(),
            name: None,
        };
        Expression::Object { ty, values }
    }

    fn from_array_node(node: syntax_nodes::Array, ctx: &mut LookupCtx) -> Expression {
        let mut values: Vec<Expression> =
            node.Expression().map(|e| Expression::from_expression_node(e, ctx)).collect();

        // FIXME: what's the type of an empty array ?
        let element_ty =
            Self::common_target_type_for_type_list(values.iter().map(|expr| expr.ty()));

        for e in values.iter_mut() {
            *e = core::mem::replace(e, Expression::Invalid).maybe_convert_to(
                element_ty.clone(),
                &node,
                ctx.diag,
            );
        }

        Expression::Array { element_ty, values }
    }

    /// This function is used to find a type that's suitable for casting each instance of a bunch of expressions
    /// to a type that captures most aspects. For example for an array of object literals the result is a merge of
    /// all seen fields.
    fn common_target_type_for_type_list(types: impl Iterator<Item = Type>) -> Type {
        types.fold(Type::Invalid, |target_type, expr_ty| {
            if target_type == expr_ty {
                target_type
            } else if target_type == Type::Invalid {
                expr_ty
            } else {
                match (target_type, expr_ty) {
                    (
                        Type::Object { fields: mut result_fields, name: result_name },
                        Type::Object { fields: elem_fields, name: elem_name },
                    ) => {
                        for (elem_name, elem_ty) in elem_fields.into_iter() {
                            match result_fields.entry(elem_name) {
                                std::collections::btree_map::Entry::Vacant(free_entry) => {
                                    free_entry.insert(elem_ty);
                                }
                                std::collections::btree_map::Entry::Occupied(
                                    mut existing_field,
                                ) => {
                                    *existing_field.get_mut() =
                                        Self::common_target_type_for_type_list(
                                            [existing_field.get().clone(), elem_ty].iter().cloned(),
                                        );
                                }
                            }
                        }
                        Type::Object { name: result_name.or(elem_name), fields: result_fields }
                    }
                    (target_type, expr_ty) => {
                        if expr_ty.can_convert(&target_type) {
                            target_type
                        } else if target_type.can_convert(&expr_ty) {
                            expr_ty
                        } else {
                            Type::Invalid
                        }
                    }
                }
            }
        })
    }
}

fn min_max_macro(
    node: NodeOrTokenWithSourceFile,
    op: char,
    args: Vec<(Expression, NodeOrTokenWithSourceFile)>,
    diag: &mut BuildDiagnostics,
) -> Expression {
    if args.is_empty() {
        diag.push_error("Needs at least one argument".into(), &node);
        return Expression::Invalid;
    }
    let mut args = args.into_iter();
    let (mut base, arg_node) = args.next().unwrap();
    let ty = match base.ty() {
        Type::Float32 => Type::Float32,
        // In case there are other floats, we don't want to conver tthe result to int
        Type::Int32 => Type::Float32,
        Type::Length => Type::Length,
        Type::LogicalLength => Type::LogicalLength,
        Type::Duration => Type::Duration,
        Type::Percent => Type::Float32,
        _ => {
            diag.push_error("Invalid argument type".into(), &arg_node);
            return Expression::Invalid;
        }
    };
    for (next, arg_node) in args {
        let rhs = next.maybe_convert_to(ty.clone(), &arg_node, diag);
        static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let n1 = format!("minmax_lhs{}", id);
        let n2 = format!("minmax_rhs{}", id);
        let a1 = Box::new(Expression::ReadLocalVariable { name: n1.clone(), ty: ty.clone() });
        let a2 = Box::new(Expression::ReadLocalVariable { name: n2.clone(), ty: ty.clone() });
        base = Expression::CodeBlock(vec![
            Expression::StoreLocalVariable { name: n1, value: Box::new(base) },
            Expression::StoreLocalVariable { name: n2, value: Box::new(rhs) },
            Expression::Condition {
                condition: Box::new(Expression::BinaryExpression {
                    lhs: a1.clone(),
                    rhs: a2.clone(),
                    op,
                }),
                true_expr: a1,
                false_expr: a2,
            },
        ]);
    }
    base
}

fn continue_lookup_within_element(
    elem: &ElementRc,
    it: &mut impl Iterator<Item = crate::parser::SyntaxTokenWithSourceFile>,
    node: SyntaxNodeWithSourceFile,
    ctx: &mut LookupCtx,
) -> Expression {
    let second = if let Some(second) = it.next() {
        second
    } else if matches!(ctx.property_type, Type::ElementReference) {
        return Expression::ElementReference(Rc::downgrade(elem));
    } else {
        ctx.diag.push_error("Cannot take reference of an element".into(), &node);
        return Expression::Invalid;
    };
    let prop_name = crate::parser::normalize_identifier(second.text().as_str());

    let p = elem.borrow().lookup_property(&prop_name);
    if p.is_property_type() {
        let prop = Expression::PropertyReference(NamedReference {
            element: Rc::downgrade(elem),
            name: prop_name,
        });
        maybe_lookup_object(prop, it, ctx)
    } else if matches!(p, Type::Callback{..}) {
        if let Some(x) = it.next() {
            ctx.diag.push_error("Cannot access fields of callback".into(), &x)
        }
        Expression::CallbackReference(NamedReference {
            element: Rc::downgrade(elem),
            name: prop_name,
        })
    } else if matches!(p, Type::Function{..}) {
        let member = elem.borrow().base_type.lookup_member_function(&prop_name);
        Expression::MemberFunction {
            base: Box::new(Expression::ElementReference(Rc::downgrade(elem))),
            base_node: node.into(),
            member: Box::new(member),
        }
    } else {
        let mut err = |extra: &str| {
            let what = match &elem.borrow().base_type {
                Type::Void => {
                    let global = elem.borrow().enclosing_component.upgrade().unwrap();
                    assert!(global.is_global());
                    format!("'{}'", global.id)
                }
                Type::Component(c) => format!("Element '{}'", c.id),
                Type::Builtin(b) => format!("Element '{}'", b.name),
                _ => unreachable!(),
            };
            ctx.diag.push_error(
                format!("{} does not have a property '{}'.{}", what, second.text(), extra),
                &second,
            );
        };
        if let Some(minus_pos) = second.text().find('-') {
            // Attempt to recover if the user wanted to write "-"
            if elem.borrow().lookup_property(&second.text()[0..minus_pos]) != Type::Invalid {
                err(" Use space before the '-' if you meant a substraction.");
                return Expression::Invalid;
            }
        }
        err("");
        Expression::Invalid
    }
}

fn maybe_lookup_object(
    mut base: Expression,
    it: impl Iterator<Item = crate::parser::SyntaxTokenWithSourceFile>,
    ctx: &mut LookupCtx,
) -> Expression {
    fn error_or_try_minus(
        ctx: &mut LookupCtx,
        ident: crate::parser::SyntaxTokenWithSourceFile,
        lookup: impl Fn(&str) -> bool,
    ) -> Expression {
        if let Some(minus_pos) = ident.text().find('-') {
            if lookup(&ident.text()[0..minus_pos]) {
                ctx.diag.push_error(format!("Cannot access the field '{}'. Use space before the '-' if you meant a substraction.", ident.text()), &ident);
                return Expression::Invalid;
            }
        }
        ctx.diag.push_error(format!("Cannot access the field '{}'", ident.text()), &ident);
        Expression::Invalid
    }

    for next in it {
        let next_str = crate::parser::normalize_identifier(next.text().as_str());
        match base.ty() {
            Type::Object { fields, .. } => {
                if fields.get(next_str.as_str()).is_some() {
                    base = Expression::ObjectAccess {
                        base: Box::new(std::mem::replace(&mut base, Expression::Invalid)),
                        name: next_str,
                    }
                } else {
                    return error_or_try_minus(ctx, next, |x| fields.get(x).is_some());
                }
            }
            Type::Component(c) => {
                let prop_ty = c.root_element.borrow().lookup_property(next_str.as_str());
                if prop_ty != Type::Invalid {
                    base = Expression::ObjectAccess {
                        base: Box::new(std::mem::replace(&mut base, Expression::Invalid)),
                        name: next.to_string(),
                    }
                } else {
                    return error_or_try_minus(ctx, next, |x| {
                        c.root_element.borrow().lookup_property(x) != Type::Invalid
                    });
                }
            }
            Type::String => {
                return Expression::MemberFunction {
                    base: Box::new(base),
                    base_node: next.clone().into(), // Note that this is not the base_node, but the function's node
                    member: Box::new(match next_str.as_str() {
                        "is_float" => {
                            Expression::BuiltinFunctionReference(BuiltinFunction::StringIsFloat)
                        }
                        "to_float" => {
                            Expression::BuiltinFunctionReference(BuiltinFunction::StringToFloat)
                        }
                        _ => {
                            ctx.diag.push_error("Cannot access fields of string".into(), &next);
                            return Expression::Invalid;
                        }
                    }),
                };
            }
            _ => {
                ctx.diag.push_error("Cannot access fields of property".into(), &next);
                return Expression::Invalid;
            }
        }
    }
    base
}

fn parse_color_literal(str: &str) -> Option<u32> {
    if !str.starts_with('#') {
        return None;
    }
    if !str.is_ascii() {
        return None;
    }
    let str = &str[1..];
    let (r, g, b, a) = match str.len() {
        3 => (
            u8::from_str_radix(&str[0..=0], 16).ok()? * 0x11,
            u8::from_str_radix(&str[1..=1], 16).ok()? * 0x11,
            u8::from_str_radix(&str[2..=2], 16).ok()? * 0x11,
            255u8,
        ),
        4 => (
            u8::from_str_radix(&str[0..=0], 16).ok()? * 0x11,
            u8::from_str_radix(&str[1..=1], 16).ok()? * 0x11,
            u8::from_str_radix(&str[2..=2], 16).ok()? * 0x11,
            u8::from_str_radix(&str[3..=3], 16).ok()? * 0x11,
        ),
        6 => (
            u8::from_str_radix(&str[0..2], 16).ok()?,
            u8::from_str_radix(&str[2..4], 16).ok()?,
            u8::from_str_radix(&str[4..6], 16).ok()?,
            255u8,
        ),
        8 => (
            u8::from_str_radix(&str[0..2], 16).ok()?,
            u8::from_str_radix(&str[2..4], 16).ok()?,
            u8::from_str_radix(&str[4..6], 16).ok()?,
            u8::from_str_radix(&str[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some((a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | (b as u32))
}

#[test]
fn test_parse_color_literal() {
    assert_eq!(parse_color_literal("#abc"), Some(0xffaabbcc));
    assert_eq!(parse_color_literal("#ABC"), Some(0xffaabbcc));
    assert_eq!(parse_color_literal("#AbC"), Some(0xffaabbcc));
    assert_eq!(parse_color_literal("#AbCd"), Some(0xddaabbcc));
    assert_eq!(parse_color_literal("#01234567"), Some(0x67012345));
    assert_eq!(parse_color_literal("#012345"), Some(0xff012345));
    assert_eq!(parse_color_literal("_01234567"), None);
    assert_eq!(parse_color_literal("→↓←"), None);
    assert_eq!(parse_color_literal("#→↓←"), None);
    assert_eq!(parse_color_literal("#1234567890"), None);
}

fn unescape_string(string: &str) -> Option<String> {
    if !string.starts_with('"') || !string.ends_with('"') {
        return None;
    }
    let string = &string[1..(string.len() - 1)];
    // TODO: remove slashes
    Some(string.into())
}

fn parse_number_literal(s: String) -> Result<Expression, String> {
    let bytes = s.as_bytes();
    let mut end = 0;
    while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b'.') {
        end += 1;
    }
    let val = s[..end].parse().map_err(|_| "Cannot parse number literal".to_owned())?;
    let unit = s[end..].parse().map_err(|_| "Invalid unit".to_owned())?;
    Ok(Expression::NumberLiteral(val, unit))
}

#[test]
fn test_parse_number_literal() {
    fn doit(s: &str) -> Result<(f64, Unit), String> {
        parse_number_literal(s.into()).map(|e| match e {
            Expression::NumberLiteral(a, b) => (a, b),
            _ => panic!(),
        })
    }

    assert_eq!(doit("10"), Ok((10., Unit::None)));
    assert_eq!(doit("10phx"), Ok((10., Unit::Phx)));
    assert_eq!(doit("10.0phx"), Ok((10., Unit::Phx)));
    assert_eq!(doit("10.0"), Ok((10., Unit::None)));
    assert_eq!(doit("1.1phx"), Ok((1.1, Unit::Phx)));
    assert_eq!(doit("10.10"), Ok((10.10, Unit::None)));
    assert_eq!(doit("10000000"), Ok((10000000., Unit::None)));
    assert_eq!(doit("10000001phx"), Ok((10000001., Unit::Phx)));

    let wrong_unit = Err("Invalid unit".to_owned());
    let cannot_parse = Err("Cannot parse number literal".to_owned());
    assert_eq!(doit("10000001 phx"), wrong_unit);
    assert_eq!(doit("12.10.12phx"), cannot_parse);
    assert_eq!(doit("12.12oo"), wrong_unit);
    assert_eq!(doit("12.12€"), wrong_unit);
}
