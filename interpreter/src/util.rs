use std::{fs::File, io::BufReader};

use regex::Regex;
use serde_json::Value;

use crate::{Heap, StackValue};

pub fn get_class_and_method(arg: &str) -> (String, String) {
    let re = Regex::new(r"(.*)\.(.*):(.*)").unwrap();
    let caps = re.captures(arg).unwrap();

    let classname = caps.get(1).unwrap().as_str().replace('.', "/");
    let methodname = caps.get(2).unwrap().as_str().to_string();
    (classname, methodname)
}

pub fn read_json(classname: &str) -> Value {
    let path = "target/decompiled/".to_string() + classname + ".json";
    eprintln!("path {}", path);
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);
    serde_json::from_reader(reader).unwrap()
}

pub fn empty_input() -> (Vec<StackValue>, Heap) {
    (vec![], Heap { heap: vec![] })
}
