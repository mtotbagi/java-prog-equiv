use std::collections::HashMap;

use serde_json::Value;
use z3::ast::Float;

use crate::{
    Access, BitOp, Cond, Constant, Instruction, Op, SimpleRef, StackType, StackValue,
    java_class::{Class, Method, MethodId},
    java_types::SimpleType,
    z3_false,
};

pub trait FromJson {
    fn from_json(json: &Value) -> Self;
}

impl FromJson for u32 {
    fn from_json(json: &Value) -> u32 {
        let Value::Number(n) = json else {
            panic!("Invalid json, {}", json)
        };
        n.as_u64().expect("Integer expected") as u32
    }
}

impl FromJson for i64 {
    fn from_json(json: &Value) -> i64 {
        let Value::Number(n) = json else {
            panic!("Invalid json, {}", json)
        };
        n.as_i64().expect("Integer expected")
    }
}

impl FromJson for i32 {
    fn from_json(json: &Value) -> i32 {
        let Value::Number(n) = json else {
            panic!("Invalid json")
        };
        n.as_i64().expect("Integer expected") as i32
    }
}

impl FromJson for f32 {
    fn from_json(json: &Value) -> f32 {
        let Value::Number(n) = json else {
            panic!("Invalid json")
        };
        n.as_f64().expect("Float expected") as f32
    }
}

impl FromJson for f64 {
    fn from_json(json: &Value) -> f64 {
        let Value::Number(n) = json else {
            panic!("Invalid json")
        };
        if let Some(f) = n.as_f64() {
            return f;
        }

        // AI fix to a parsing problem, unsure if this is what we need to do.

        // The decompiler prints e.g. Double.MAX_VALUE with too few significant
        // digits (1.797693134862316e308), which rounds past f64::MAX to infinity,
        // and serde_json's as_f64 rejects non-finite values. JSON cannot encode
        // a real infinity, so an overflowing literal must mean +-f64::MAX.
        let f: f64 = n
            .to_string()
            .parse()
            .unwrap_or_else(|_| panic!("Double expected, got {}", n));
        if f.is_infinite() {
            f64::MAX.copysign(f)
        } else {
            f
        }
    }
}

impl FromJson for StackValue {
    fn from_json(json: &Value) -> Self {
        if json.is_null() {
            return Self::Ref(
                None,
                z3::ast::Bool::from_bool(false),
                z3::ast::BV::from_u64(0, 32),
            );
        }

        let Value::String(s) = &json["type"] else {
            panic!("Invalid json {}", json)
        };
        match s.as_str() {
            "int" | "integer" => {
                let val = i32::from_json(&json["value"]);
                let ast = z3::ast::BV::from_i64(val as i64, 32);
                Self::Int(val, ast)
            }
            "float" => {
                let val = f32::from_json(&json["value"]);
                Self::Float(val, Float::from_f32(val))
            }
            "ref" => todo!(),
            "double" => {
                let val = f64::from_json(&json["value"]);
                Self::Double(val, Float::from_f64(val))
            }
            "long" => {
                let val = i64::from_json(&json["value"]);
                let ast = z3::ast::BV::from_i64(val, 64);
                Self::Long(val, ast)
            }
            _ => panic!("Invalid json {}", json),
        }
    }
}

impl FromJson for Constant {
    fn from_json(json: &Value) -> Self {
        if json.is_null() {
            return Self::Stack(StackValue::Ref(
                None,
                z3::ast::Bool::from_bool(false),
                z3::ast::BV::from_u64(0, 32),
            ));
        }

        let Value::String(s) = &json["type"] else {
            panic!("Invalid json {}", json)
        };
        if s == "string" {
            let val = json["value"].as_str().unwrap().to_string();
            Self::Heap(crate::util::create_string(&val, false))
        } else {
            Self::Stack(StackValue::from_json(json))
        }
    }
}

impl FromJson for StackType {
    fn from_json(json: &Value) -> Self {
        let Value::String(s) = &json else {
            panic!("Invalid json")
        };
        match s.as_str() {
            "int" => Self::Int,
            "float" => Self::Float,
            "ref" => Self::Ref,
            "class" => Self::Ref,
            "double" => Self::Double,
            "long" => Self::Long,
            _ => panic!("Invalid json"),
        }
    }
}

impl FromJson for SimpleType {
    fn from_json(json: &Value) -> Self {
        if let Value::String(kind) = &json["kind"] {
            match kind.as_str() {
                "array" => {
                    return SimpleType::SimpleRef(SimpleRef::Array {
                        ty: SimpleType::from_json(&json["type"]).to_array_type(),
                    });
                }
                "class" => {
                    return SimpleType::SimpleRef(SimpleRef::Class {
                        name: json["name"].as_str().unwrap().to_string(),
                    });
                }
                _ => todo!(),
            }
        }
        // dbg!(json);
        let s = if json.is_string() {
            json.as_str().unwrap()
        } else {
            json["base"].as_str().unwrap()
        };
        match s {
            "int" => Self::Int,
            "float" => Self::Float,
            "bool" | "boolean" => Self::Boolean,
            "byte" => Self::Byte,
            "char" => Self::Char,
            "short" => Self::Short,
            "double" => Self::Double,
            "long" => Self::Long,
            _ => panic!("Invalid json {}", json),
        }
    }
}

impl FromJson for Option<SimpleType> {
    fn from_json(json: &Value) -> Self {
        if json.is_null() {
            return None;
        }
        if let Value::String(kind) = &json["kind"] {
            match kind.as_str() {
                "array" => {
                    return Some(SimpleType::SimpleRef(SimpleRef::Array {
                        ty: SimpleType::from_json(&json["type"]).to_array_type(),
                    }));
                }
                "class" => {
                    return Some(SimpleType::SimpleRef(SimpleRef::Class {
                        name: json["name"].as_str().unwrap().to_string(),
                    }));
                }
                _ => todo!(),
            }
        }
        // eprintln!("return type json: {}", json);
        let s = if json.is_string() {
            json.as_str().unwrap()
        } else if json["base"].is_string() {
            json["base"].as_str().unwrap()
        } else if json["type"]["base"].is_string() {
            json["type"]["base"].as_str().unwrap()
        } else {
            return None;
        };
        match s {
            "int" => Some(SimpleType::Int),
            "float" => Some(SimpleType::Float),
            "bool" | "boolean" => Some(SimpleType::Boolean),
            "byte" => Some(SimpleType::Byte),
            "char" => Some(SimpleType::Char),
            "short" => Some(SimpleType::Short),
            "double" => Some(SimpleType::Double),
            "long" => Some(SimpleType::Long),
            _ => panic!("Invalid json {}", json),
        }
    }
}

impl FromJson for Option<StackType> {
    fn from_json(json: &Value) -> Self {
        let Value::String(s) = &json else {
            // eprintln!("{}", json);
            if json.is_null() {
                return None;
            } else {
                panic!("Invalid json")
            }
        };
        match s.as_str() {
            "int" => Some(StackType::Int),
            "float" => Some(StackType::Float),
            "ref" => Some(StackType::Ref),
            "class" => Some(StackType::Ref),
            "double" => Some(StackType::Double),
            "long" => Some(StackType::Long),
            _ => panic!("Invalid json"),
        }
    }
}

impl FromJson for Instruction {
    fn from_json(json: &Value) -> Self {
        let Value::String(s) = &json["opr"] else {
            panic!("Invalid json")
        };

        match s.as_str() {
            "ifz" => Self::Ifz {
                cond: Cond::from_json(&json["condition"]),
                target: u32::from_json(&json["target"]),
            },
            "if" => Self::If {
                cond: Cond::from_json(&json["condition"]),
                target: u32::from_json(&json["target"]),
            },
            "load" => Self::Load {
                ty: StackType::from_json(&json["type"]),
                index: u32::from_json(&json["index"]),
            },
            "store" => Self::Store {
                ty: StackType::from_json(&json["type"]),
                index: u32::from_json(&json["index"]),
            },
            "push" => Self::Push {
                value: Constant::from_json(&json["value"]),
            },
            "return" => Self::Return {
                ty: Option::<StackType>::from_json(&json["type"]),
            },
            "get" => {
                let is_static = json["static"].as_bool().unwrap();
                let field_name = json["field"]["name"].as_str().unwrap().to_string();
                let class_name = json["field"]["class"].as_str().unwrap().to_string();
                let field_ty = SimpleType::from_json(&json["field"]["type"]);
                Self::Get {
                    is_static,
                    class_name,
                    field_name,
                    field_ty,
                }
            }
            "new" => Self::New {
                class: json["class"].as_str().unwrap().to_string(),
            },
            "dup" => Self::Dup {
                words: json["words"].as_u64().unwrap() as u32,
            },
            "invoke" => Self::Invoke {
                access: Access::from_json(&json["access"]),
                method_id: MethodId::from_json(&json["method"]),
                simple_ref: SimpleRef::from_json(&json["method"]["ref"]),
            },
            "throw" => Self::Throw,
            "binary" => Self::Binary {
                op: Op::from_json(&json["operant"]),
                ty: StackType::from_json(&json["type"]),
            },
            "goto" => Self::Goto {
                target: u32::from_json(&json["target"]),
            },
            "newarray" => Self::NewArray {
                dim: u32::from_json(&json["dim"]),
                ty: SimpleType::from_json(&json["type"]).to_array_type(),
            },
            "array_store" => Self::ArrayStore {
                ty: SimpleType::from_json(&json["type"]).to_array_type(),
            },
            "arraylength" => Self::ArrayLength,
            "array_load" => Self::ArrayLoad {
                ty: SimpleType::from_json(&json["type"]).to_array_type(),
            },
            "incr" => Self::Incr {
                index: u32::from_json(&json["index"]),
                amount: i32::from_json(&json["amount"]),
            },
            "negate" => Self::Neg {
                ty: StackType::from_json(&json["type"]),
            },
            "cast" => Self::Cast {
                from: StackType::from_json(&json["from"]),
                to: SimpleType::from_json(&json["to"]),
            },
            "comparefloating" => Self::CompareFloat {
                on_nan: i32::from_json(&json["onnan"]),
                double: json["type"] == Value::String("double".to_string()),
            },
            "put" => {
                let is_static = json["static"].as_bool().unwrap();
                let field_name = json["field"]["name"].as_str().unwrap().to_string();
                let class_name = json["field"]["class"].as_str().unwrap().to_string();
                let field_ty = SimpleType::from_json(&json["field"]["type"]);
                Self::Put {
                    is_static,
                    class_name,
                    field_name,
                    field_ty,
                }
            }
            "bitopr" => Self::BitOpr {
                op: BitOp::from_json(&json["operant"]),
            },
            "comparelongs" => Self::CompareLong,
            "dup_x2" => Self::DupX2,
            _ => unimplemented!("{}", json),
        }
    }
}

impl FromJson for BitOp {
    fn from_json(json: &Value) -> Self {
        match json.as_str().unwrap() {
            "shl" => BitOp::Shl,
            "shr" => BitOp::Shr,
            "ushr" => BitOp::Ushr,
            "and" => BitOp::And,
            "or" => BitOp::Or,
            "xor" => BitOp::Xor,
            _ => panic!("Invalid json {json}"),
        }
    }
}

impl FromJson for Op {
    fn from_json(json: &Value) -> Self {
        match json.as_str().unwrap() {
            "add" => Self::Add,
            "sub" => Self::Sub,
            "mul" => Self::Mul,
            "div" => Self::Div,
            "rem" => Self::Rem,
            _ => panic!("Invalid json"),
        }
    }
}

impl FromJson for Access {
    fn from_json(json: &Value) -> Self {
        let access = json.as_str().expect("Invalid json");
        match access {
            "special" => Self::Special,
            "static" => Self::Static,
            "dynamic" => todo!(),
            "interface" => todo!(),
            "virtual" => Self::Virtual,
            _ => panic!("Invalid json"),
        }
    }
}

impl FromJson for SimpleRef {
    fn from_json(json: &Value) -> Self {
        let kind = json["kind"].as_str().expect("Invalid json");
        match kind {
            "class" => Self::Class {
                name: json["name"].as_str().expect("Invalid json").to_string(),
            },
            "array" => todo!(),
            _ => panic!("Invalid json"),
        }
    }
}

impl FromJson for Cond {
    fn from_json(json: &Value) -> Self {
        let Value::String(s) = &json else {
            panic!("Invalid json")
        };
        match s.as_str() {
            "ne" => Self::Ne,
            "eq" => Self::Eq,
            "lt" => Self::Lt,
            "le" => Self::Le,
            "gt" => Self::Gt,
            "ge" => Self::Ge,
            "is" => Self::Is,
            "isnot" => Self::IsNot,
            _ => panic!("Invalid json: {}", json),
        }
    }
}

impl FromJson for MethodId {
    fn from_json(json: &Value) -> Self {
        let name = json["name"].as_str().unwrap().to_string();
        // eprintln!("parsing fn: {name}");

        let params: Box<_> = if json["params"].is_null() {
            json["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(SimpleType::from_json)
                .collect()
        } else {
            json["params"]
                .as_array()
                .unwrap()
                .iter()
                .map(|json| SimpleType::from_json(&json["type"]))
                .collect()
        };

        let ret_ty = Option::<SimpleType>::from_json(&json["returns"]);
        // eprintln!("return type: {:?}", ret_ty);
        // if name == "multiError" {
        //     panic!()
        // }
        MethodId {
            name,
            params,
            ret_ty,
        }
    }
}

impl FromJson for Method {
    fn from_json(json: &Value) -> Self {
        let instructions: Box<_> = json["code"]["bytecode"]
            .as_array()
            .unwrap()
            .iter()
            .map(Instruction::from_json)
            .collect();

        let is_static = json["access"]
            .as_array()
            .is_some_and(|access| access.iter().any(|a| a == "static"));

        Method {
            id: MethodId::from_json(json),
            instructions,
            is_static,
        }
    }
}

impl FromJson for Class {
    fn from_json(json: &Value) -> Self {
        let name = json["name"].as_str().unwrap().to_string();
        let methods: Box<_> = json["methods"]
            .as_array()
            .unwrap()
            .iter()
            .map(Method::from_json)
            .map(|m| dbg!(m))
            .collect();
        let fields_iter = json["fields"].as_array().unwrap().iter();
        let non_static_fields: Box<_> = fields_iter
            .clone()
            .filter(|v| {
                !v["access"]
                    .as_array()
                    .unwrap()
                    .contains(&Value::String("static".to_string()))
            })
            .map(|v| {
                (
                    v["name"].as_str().unwrap().to_string(),
                    SimpleType::from_json(&v["type"]),
                )
            })
            .collect();

        let mut static_fields = HashMap::new();
        fields_iter
            .filter(|v| {
                v["access"]
                    .as_array()
                    .unwrap()
                    .contains(&Value::String("static".to_string()))
            })
            .map(|v| {
                (
                    v["name"].as_str().unwrap().to_string(),
                    SimpleType::from_json(&v["type"]),
                )
            })
            .for_each(|(name, ty)| {
                let default_val = match ty {
                    SimpleType::Int => StackValue::Int(0, z3::ast::BV::from_i64(0, 32)),
                    SimpleType::Float => StackValue::Float(0.0, Float::from_f32(0.0)),
                    SimpleType::Long => StackValue::Long(0, z3::ast::BV::from_i64(0, 64)),
                    SimpleType::Double => StackValue::Double(0.0, Float::from_f64(0.0)),
                    SimpleType::SimpleRef(_) => {
                        StackValue::Ref(None, z3_false(), z3::ast::BV::from_i64(0, 32))
                    }
                    _ => panic!(),
                };
                static_fields.insert(name, default_val);
            });
        Class {
            name,
            methods,
            non_static_fields,
            static_fields,
        }
    }
}
