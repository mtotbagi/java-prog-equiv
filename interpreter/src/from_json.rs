use serde_json::Value;

use crate::{
    Access, Cond, Instruction, Op, SimpleRef, StackType, StackValue,
    java_class::{Class, Method, MethodId},
    java_types::SimpleType,
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

impl FromJson for i32 {
    fn from_json(json: &Value) -> i32 {
        let Value::Number(n) = json else {
            panic!("Invalid json")
        };
        n.as_i64().expect("Integer expected") as i32
    }
}

impl FromJson for StackValue {
    fn from_json(json: &Value) -> Self {
        if json.is_null() {
            return Self::Ref(None);
        }

        let Value::String(s) = &json["type"] else {
            panic!("Invalid json {}", json)
        };
        match s.as_str() {
            "int" => Self::Int(i32::from_json(&json["value"])),
            "integer" => Self::Int(i32::from_json(&json["value"])),
            "float" => todo!(),
            "ref" => todo!(),
            _ => panic!("Invalid json {}", json),
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
            _ => panic!("Invalid json"),
        }
    }
}

impl FromJson for SimpleType {
    fn from_json(json: &Value) -> Self {
        if let Value::String(kind) = &json["kind"] {
            match kind.as_str() {
                "array" => {
                    return SimpleType::SimpleRef(Box::new(SimpleRef::Array {
                        ty: SimpleType::from_json(&json["type"]),
                    }));
                }
                _ => todo!(),
            }
        }
        dbg!(json);
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
                    return Some(SimpleType::SimpleRef(Box::new(SimpleRef::Array {
                        ty: SimpleType::from_json(&json["type"]),
                    })));
                }
                _ => todo!(),
            }
        }
        eprintln!("return type json: {}", json);
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
            _ => panic!("Invalid json {}", json),
        }
    }
}

impl FromJson for Option<StackType> {
    fn from_json(json: &Value) -> Self {
        let Value::String(s) = &json else {
            eprintln!("{}", json);
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
                value: StackValue::from_json(&json["value"]),
            },
            "return" => Self::Return {
                ty: Option::<StackType>::from_json(&json["type"]),
            },
            "get" => {
                if json["field"]["name"] != "$assertionsDisabled" {
                    todo!()
                }
                Self::Get
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
                ty: SimpleType::from_json(&json["type"]),
            },
            "array_store" => Self::ArrayStore {
                ty: SimpleType::from_json(&json["type"]),
            },
            "arraylength" => Self::ArrayLength,
            "array_load" => Self::ArrayLoad {
                ty: SimpleType::from_json(&json["type"]),
            },
            "incr" => Self::Incr {
                index: u32::from_json(&json["index"]),
                amount: i32::from_json(&json["amount"]),
            },
            "negate" => Self::Neg {
                ty: StackType::from_json(&json["type"]),
            },
            // As best as I can determine, cast doesn't actually do anything
            "cast" => Self::NoOp,
            _ => unimplemented!("{}", json),
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
            "virtual" => todo!(),
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
        eprintln!("parsing fn: {name}");

        let params: Box<_> = if json["params"].is_null() {
            json["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|json| SimpleType::from_json(&json))
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

        Method {
            id: MethodId::from_json(json),
            instructions,
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
            // Skip the <clinit> method as I have no idea how push would work with a class
            .filter(|m| m["name"] != "<clinit>")
            .map(Method::from_json)
            .collect();
        Class { name, methods }
    }
}
