//! A miter for each case: a Java program calling both versions of the
//! compared method on the same inputs and asserting that they agree.
//!
//! It is shared by the tools that check a program rather than a pair
//! (bounded model checkers, symbolic executors, fuzzers):
//!
//! - `eqbench.v1.oldV` and `eqbench.v2.newV` are the two versions, moved to
//!   neutral packages so that no tool sees the `Eq` / `Neq` of the case, each
//!   with a static `eqbenchCall` method calling the compared one (which may
//!   be private, or an instance method);
//! - `eqbench.Miter.check(<params>)` asserts that both versions return the
//!   same value (NaN equal to NaN), or both throw an exception;
//! - `eqbench.Miter.main` calls `check` on nondeterministic inputs from
//!   `org.sosy_lab.sv_benchmarks.Verifier`, as SV-COMP verifiers expect.
//!
//! Only methods with primitive parameters and a primitive result, of the
//! same signature in both versions, get a miter.

use std::fmt::Write;

use serde::Deserialize;
use serde_json::Value;

/// Class of the miter, and the package of each version.
pub const MITER_CLASS: &str = "eqbench.Miter";
pub const OLD_PACKAGE: &str = "eqbench.v1";
pub const NEW_PACKAGE: &str = "eqbench.v2";
const BRIDGE: &str = "eqbenchCall";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prim {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
}

impl Prim {
    fn from_jvm2json(base: &str) -> Option<Prim> {
        Some(match base {
            "boolean" => Prim::Boolean,
            "byte" => Prim::Byte,
            "char" => Prim::Char,
            "short" => Prim::Short,
            "int" => Prim::Int,
            "long" => Prim::Long,
            "float" => Prim::Float,
            "double" => Prim::Double,
            _ => return None,
        })
    }

    pub fn java(self) -> &'static str {
        match self {
            Prim::Boolean => "boolean",
            Prim::Byte => "byte",
            Prim::Char => "char",
            Prim::Short => "short",
            Prim::Int => "int",
            Prim::Long => "long",
            Prim::Float => "float",
            Prim::Double => "double",
        }
    }

    fn nondet(self) -> &'static str {
        match self {
            Prim::Boolean => "nondetBoolean",
            Prim::Byte => "nondetByte",
            Prim::Char => "nondetChar",
            Prim::Short => "nondetShort",
            Prim::Int => "nondetInt",
            Prim::Long => "nondetLong",
            Prim::Float => "nondetFloat",
            Prim::Double => "nondetDouble",
        }
    }

    fn is_floating(self) -> bool {
        matches!(self, Prim::Float | Prim::Double)
    }
}

/// What the miter needs to know about the compared method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub params: Vec<Prim>,
    pub result: Prim,
    pub is_static: bool,
}

/// The part of a jvm2json class the miter needs. The code of the methods is
/// skipped without being parsed: some hold numbers out of the range of `f64`.
#[derive(Debug, Deserialize)]
pub struct Class {
    pub methods: Vec<Method>,
}

#[derive(Debug, Deserialize)]
pub struct Method {
    pub name: String,
    #[serde(default)]
    pub access: Vec<String>,
    #[serde(default)]
    pub params: Vec<Typed>,
    pub returns: Typed,
}

#[derive(Debug, Deserialize)]
pub struct Typed {
    #[serde(rename = "type")]
    pub ty: Value,
}

/// Signature of `method` in a class. The error is why the case gets no miter.
pub fn signature(class: &Class, method: &str) -> Result<Signature, String> {
    let found: Vec<&Method> = class.methods.iter().filter(|m| m.name == method).collect();
    let m = match found.as_slice() {
        [] => return Err(format!("no method `{method}` in the class")),
        [m] => *m,
        _ => return Err(format!("`{method}` is overloaded")),
    };
    let params = m
        .params
        .iter()
        .map(|p| primitive(&p.ty).map_err(|ty| format!("`{method}` takes {ty}")))
        .collect::<Result<_, _>>()?;
    let result = match &m.returns.ty {
        Value::Null => return Err(format!("`{method}` returns void")),
        ty => primitive(ty).map_err(|ty| format!("`{method}` returns {ty}"))?,
    };
    let is_static = m.access.iter().any(|a| a == "static");
    if !is_static {
        let default_constructor = class
            .methods
            .iter()
            .any(|m| m.name == "<init>" && m.params.is_empty());
        if !default_constructor {
            return Err("no constructor without parameters".to_string());
        }
    }
    Ok(Signature {
        params,
        result,
        is_static,
    })
}

/// The primitive type `ty`, or a short description of what it is instead.
fn primitive(ty: &Value) -> Result<Prim, String> {
    if let Some(base) = ty["base"].as_str() {
        return Prim::from_jvm2json(base).ok_or_else(|| base.to_string());
    }
    Err(match ty["kind"].as_str() {
        Some("class") => format!("an object ({})", ty["name"].as_str().unwrap_or("?")),
        Some(kind) => format!("an {kind}"),
        None => "a non-primitive".to_string(),
    })
}

/// `source` of class `class` with `eqbenchCall`, a static method calling
/// `method`, appended to the class.
pub fn add_bridge(source: &str, class: &str, method: &str, sig: &Signature) -> Option<String> {
    let close = class_end(source, class)?;
    let params = parameters(sig);
    let args = arguments(sig);
    let target = if sig.is_static {
        format!("{class}.{method}")
    } else {
        format!("new {class}().{method}")
    };
    let bridge = format!(
        "\n    public static {} {BRIDGE}({params}) {{\n        return {target}({args});\n    }}\n",
        sig.result.java()
    );
    Some(format!("{}{bridge}{}", &source[..close], &source[close..]))
}

/// Position of the brace closing class `class`. Braces are counted as is,
/// none of the cases with a miter has one in a string or a comment.
fn class_end(source: &str, class: &str) -> Option<usize> {
    let mut search = 0;
    let start = loop {
        let at = search + source[search..].find("class ")?;
        let rest = &source[at + "class ".len()..];
        let name_end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if &rest[..name_end] == class {
            break at;
        }
        search = at + 1;
    };
    let open = start + source[start..].find('{')?;
    let mut depth = 0;
    for (i, c) in source[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn parameters(sig: &Signature) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{} p{i}", p.java()))
        .collect();
    params.join(", ")
}

fn arguments(sig: &Signature) -> String {
    let args: Vec<String> = (0..sig.params.len()).map(|i| format!("p{i}")).collect();
    args.join(", ")
}

/// Source of `eqbench.Miter` for the two versions `old_class` and `new_class`.
pub fn miter_source(sig: &Signature, old_class: &str, new_class: &str) -> String {
    let ty = sig.result.java();
    let params = parameters(sig);
    let args = arguments(sig);
    let same = if sig.result.is_floating() {
        "oldResult == newResult || (oldResult != oldResult && newResult != newResult)"
    } else {
        "oldResult == newResult"
    };
    let nondet: Vec<String> = sig
        .params
        .iter()
        .map(|p| format!("Verifier.{}()", p.nondet()))
        .collect();

    let mut s = String::new();
    let _ = write!(
        s,
        r#"package eqbench;

import org.sosy_lab.sv_benchmarks.Verifier;

/** Calls both versions of the compared method on the same inputs. */
public class Miter {{
    /** Fails when the versions return different values, or one only throws. */
    public static void check({params}) {{
        {ty} oldResult = 0;
        {ty} newResult = 0;
        Throwable oldError = null;
        Throwable newError = null;
        try {{
            oldResult = {OLD_PACKAGE}.{old_class}.{BRIDGE}({args});
        }} catch (Throwable e) {{
            oldError = e;
        }}
        try {{
            newResult = {NEW_PACKAGE}.{new_class}.{BRIDGE}({args});
        }} catch (Throwable e) {{
            newError = e;
        }}
        if (oldError != null || newError != null) {{
            // Both must throw. Their classes are not compared: JBMC does not
            // model the identity of `Class` objects.
            assert oldError != null && newError != null;
        }} else {{
            assert {same};
        }}
    }}

    /** Entry point of SV-COMP verifiers. */
    public static void main(String[] args) {{
        check({nondet});
    }}
}}
"#,
        nondet = nondet.join(", ")
    );
    if sig.result == Prim::Boolean {
        s = s.replace("boolean oldResult = 0;", "boolean oldResult = false;");
        s = s.replace("boolean newResult = 0;", "boolean newResult = false;");
    }
    s
}

/// The SV-COMP API for nondeterministic values. Verifiers replace it; this
/// one draws random values, for the tools that run the miter.
pub const VERIFIER_SOURCE: &str = r#"package org.sosy_lab.sv_benchmarks;

import java.util.Random;

/** Nondeterministic values, drawn at random when the program is run. */
public final class Verifier {
    private static final Random RANDOM = new Random();

    public static void assume(boolean condition) {
        if (!condition) {
            Runtime.getRuntime().halt(1);
        }
    }

    public static boolean nondetBoolean() { return RANDOM.nextBoolean(); }
    public static byte nondetByte() { return (byte) RANDOM.nextInt(); }
    public static char nondetChar() { return (char) RANDOM.nextInt(); }
    public static short nondetShort() { return (short) RANDOM.nextInt(); }
    public static int nondetInt() { return RANDOM.nextInt(); }
    public static long nondetLong() { return RANDOM.nextLong(); }
    public static float nondetFloat() { return RANDOM.nextFloat(); }
    public static double nondetDouble() { return RANDOM.nextDouble(); }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn int() -> Value {
        json!({"type": {"base": "int"}})
    }

    fn class(methods: Value) -> Class {
        serde_json::from_value(json!({ "methods": methods })).unwrap()
    }

    #[test]
    fn reads_the_signature() {
        let c = class(json!([
            {"name": "<init>", "access": ["public"], "params": [], "returns": {"type": null}},
            {"name": "foo", "access": ["private"], "params": [int(), {"type": {"base": "double"}}],
             "returns": {"type": {"base": "int"}}},
        ]));
        assert_eq!(
            signature(&c, "foo"),
            Ok(Signature {
                params: vec![Prim::Int, Prim::Double],
                result: Prim::Int,
                is_static: false
            })
        );
        assert!(signature(&c, "bar").is_err());
    }

    #[test]
    fn rejects_what_the_miter_cannot_compare() {
        let object = json!({"type": {"kind": "class", "name": "p/Sphere"}});
        let c = class(json!([
            {"name": "v", "access": ["static"], "params": [int()], "returns": {"type": null}},
            {"name": "o", "access": ["static"], "params": [object], "returns": {"type": {"base": "int"}}},
            {"name": "i", "access": [], "params": [], "returns": {"type": {"base": "int"}}},
            {"name": "<init>", "access": [], "params": [int()], "returns": {"type": null}},
        ]));
        assert_eq!(signature(&c, "v"), Err("`v` returns void".to_string()));
        assert_eq!(
            signature(&c, "o"),
            Err("`o` takes an object (p/Sphere)".to_string())
        );
        assert_eq!(
            signature(&c, "i"),
            Err("no constructor without parameters".to_string())
        );
    }

    #[test]
    fn appends_a_bridge_to_the_class() {
        let sig = Signature {
            params: vec![Prim::Int],
            result: Prim::Int,
            is_static: false,
        };
        let source = "package p;\nclass Helper { int x() { return 1; } }\npublic class oldV {\n  private int foo(int a) { if (a > 0) { return a; } return 0; }\n}\n";
        let out = add_bridge(source, "oldV", "foo", &sig).unwrap();
        assert!(out.contains("public static int eqbenchCall(int p0) {\n        return new oldV().foo(p0);\n    }\n}\n"));
        assert!(out.starts_with("package p;\nclass Helper { int x() { return 1; } }\n"));
        assert_eq!(add_bridge(source, "newV", "foo", &sig), None);
    }

    #[test]
    fn writes_a_miter() {
        let sig = Signature {
            params: vec![Prim::Double, Prim::Int],
            result: Prim::Double,
            is_static: true,
        };
        let miter = miter_source(&sig, "oldV", "newV");
        assert!(miter.contains("public static void check(double p0, int p1)"));
        assert!(miter.contains("oldResult = eqbench.v1.oldV.eqbenchCall(p0, p1);"));
        assert!(miter.contains("newResult = eqbench.v2.newV.eqbenchCall(p0, p1);"));
        assert!(miter.contains("oldResult != oldResult && newResult != newResult"));
        assert!(miter.contains("check(Verifier.nondetDouble(), Verifier.nondetInt());"));
        let boolean = Signature {
            params: vec![],
            result: Prim::Boolean,
            is_static: true,
        };
        assert!(miter_source(&boolean, "oldV", "newV").contains("boolean oldResult = false;"));
    }
}
