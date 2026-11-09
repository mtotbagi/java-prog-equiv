use crate::{
    Array, ArrayType, ArrayValue,
    java_class::Method,
    java_types::{HeapValue, SimpleRef, SimpleType, StackValue},
    state::Heap,
};
use std::collections::HashMap;

pub fn parse_input(method: &Method, input: &str) -> (Vec<StackValue>, Heap) {
    let types = &method.id.params;
    let mut heap = Heap { heap: vec![] };

    let trimmed = input.trim();
    let inner = trimmed
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or_else(|| panic!("expected input wrapped in parens, got `{input}`"));

    let parts = split_top_level(inner);

    assert_eq!(
        parts.len(),
        types.len(),
        "argument count mismatch for `{input}`: expected {}, got {}",
        types.len(),
        parts.len()
    );

    let values = types
        .iter()
        .zip(parts)
        .map(|(t, part)| parse_input_value(t, part, &mut heap))
        .collect();

    (values, heap)
}

/// Splits a comma-separated list at the top level only, ignoring commas
/// nested inside `[...]` or `'...'` (so array/char/string literals stay intact).
fn split_top_level(s: &str) -> Vec<&str> {
    if s.trim().is_empty() {
        return vec![];
    }

    let mut parts = vec![];
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut start = 0usize;

    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        match *b as char {
            '\'' => in_quote = !in_quote,
            '[' if !in_quote => depth += 1,
            ']' if !in_quote => depth -= 1,
            ',' if !in_quote && depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts
}

fn parse_input_value(ty: &SimpleType, input: &str, heap: &mut Heap) -> StackValue {
    match ty {
        SimpleType::SimpleRef(r) => parse_ref_value(r, input, heap),
        _ => parse_array_value(&ty.to_array_type(), input).to_stack_value(),
    }
}

/// Parses a *primitive* literal directly into a HeapValue (so array elements
/// keep their proper variant, e.g. Char instead of being widened to Int).
fn parse_array_value(ty: &ArrayType, input: &str) -> ArrayValue {
    match ty {
        ArrayType::Int => ArrayValue::Int(
            input
                .parse()
                .unwrap_or_else(|_| panic!("invalid int literal `{input}`")),
            z3::ast::BV::fresh_const("int", 32),
        ),
        ArrayType::Float => ArrayValue::Float(
            input
                .parse()
                .unwrap_or_else(|_| panic!("invalid float literal `{input}`")),
        ),
        ArrayType::Byte => ArrayValue::Byte(
            input
                .parse()
                .unwrap_or_else(|_| panic!("invalid byte literal `{input}`")),
            z3::ast::BV::fresh_const("byte", 8),
        ),
        ArrayType::Short => ArrayValue::Short(
            input
                .parse()
                .unwrap_or_else(|_| panic!("invalid short literal `{input}`")),
            z3::ast::BV::fresh_const("short", 16),
        ),
        ArrayType::Char => ArrayValue::Char(
            parse_char_literal(input),
            z3::ast::BV::fresh_const("char", 16),
        ),
        ArrayType::Boolean => {
            let b: bool = input
                .parse()
                .unwrap_or_else(|_| panic!("invalid bool literal `{input}`"));
            ArrayValue::Boolean(b, z3::ast::BV::fresh_const("bool", 1))
        }
    }
}

fn parse_char_literal(input: &str) -> u16 {
    let inner = input.trim_matches('\'');
    inner
        .chars()
        .next()
        .unwrap_or_else(|| panic!("empty char literal `{input}`")) as u16
}

fn parse_ref_value(simple_ref: &SimpleRef, input: &str, heap: &mut Heap) -> StackValue {
    match simple_ref {
        SimpleRef::Array { ty } => parse_array(ty, input, heap),
        SimpleRef::Class { name } if name == "java/lang/String" => parse_string(input, heap),
        SimpleRef::Class { name } => {
            panic!("don't know how to parse a literal for class `{name}`")
        }
    }
}

/// Parses `[I:1, 2, 3]`, `[C:'h', 'e']`, `[I:]`, etc.
fn parse_array(ty: &ArrayType, input: &str, heap: &mut Heap) -> StackValue {
    let stripped = input
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or_else(|| panic!("invalid array literal `{input}`, expected `[...]`"));

    let (_tag, rest) = stripped
        .split_once(':')
        .unwrap_or_else(|| panic!("invalid array literal `{input}`, missing `:`"));

    let values: Vec<ArrayValue> = split_top_level(rest)
        .into_iter()
        .map(|elem| parse_element_heap_value(ty, elem))
        .collect();

    let idx = heap.heap.len() as u32;
    heap.heap
        .push(HeapValue::Array(Array::from_values(*ty, values)));
    StackValue::Ref(
        Some(idx),
        z3::ast::Bool::from_bool(true),
        z3::ast::BV::from_u64(idx as u64, 32),
    )
}

fn parse_element_heap_value(ty: &ArrayType, input: &str) -> ArrayValue {
    parse_array_value(ty, input)
}

/// Parses `s'hello'` into a heap-allocated `java/lang/String` object
/// whose `value` field is a char array (embedded directly for simplicity).
fn parse_string(input: &str, heap: &mut Heap) -> StackValue {
    let content = input
        .strip_prefix("s'")
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or_else(|| panic!("invalid string literal `{input}`, expected `s'...'`"));

    let chars: Vec<ArrayValue> = content
        .encode_utf16()
        .map(|c| ArrayValue::Char(c, z3::ast::BV::fresh_const("char", 16)))
        .collect();

    let mut fields = HashMap::new();
    fields.insert(
        "value".to_string(),
        HeapValue::Array(Array::from_values(ArrayType::Char, chars)),
    );

    let idx = heap.heap.len() as u32;
    heap.heap.push(HeapValue::Object {
        name: "java/lang/String".to_string(),
        fields,
    });
    StackValue::Ref(
        Some(idx),
        z3::ast::Bool::from_bool(true),
        z3::ast::BV::from_u64(idx as u64, 32),
    )
}
