use crate::{Either, ExeResult, Frame, Heap, ProgramCounter, StackValue, State};

pub trait ToSexp {
    fn to_sexp(&self) -> String;
}

impl ToSexp for StackValue {
    fn to_sexp(&self) -> String {
        let mut res = "(".to_string();
        match self {
            StackValue::Int(i) => {
                res += "int ";
                res += &i.to_string();
            }
            StackValue::Float(f) => {
                res += "float ";
                res += &f.to_string();
            }
            StackValue::Ref(Some(r)) => {
                res += "ref ";
                res += &r.to_string();
            }
            StackValue::Ref(None) => {
                res += "ref ";
                res += "null";
            }
        }
        res += ")\n";
        res
    }
}

impl ToSexp for Option<StackValue> {
    fn to_sexp(&self) -> String {
        if let Some(stack_val) = self {
            stack_val.to_sexp()
        } else {
            "(null)\n".to_string()
        }
    }
}

impl ToSexp for ProgramCounter {
    fn to_sexp(&self) -> String {
        let classname = self.class.replace("/", ".");
        ":pc \"".to_string()
            + &classname
            + "."
            + &self.method.to_string()
            + ":"
            + &self.idx.to_string()
            + "\"\n"
    }
}

impl ToSexp for Frame {
    fn to_sexp(&self) -> String {
        let mut res = "(frame\n".to_string();
        res += ":locals (\n";
        for (i, local) in self.locals.iter().enumerate() {
            res += &format!(":{:02} ", i);
            res += &local.to_sexp();
        }
        res += ")\n";
        res += ":stack (\n";
        for (i, value) in self.stack.iter().enumerate() {
            res += &format!(":{:02} ", i);
            res += &value.to_sexp();
        }
        res += ")\n";
        res += ")\n";
        res
    }
}

impl ToSexp for Heap {
    fn to_sexp(&self) -> String {
        "()\n".to_string()
    }
}

impl ToSexp for State {
    fn to_sexp(&self) -> String {
        let mut res = "(state\n".to_string();

        //heap
        res += ":heap ";
        res += &self.heap.to_sexp();

        //frames
        res += ":frames (\n";
        for (i, frame) in self.frames.iter().enumerate() {
            res += &format!(":{:02} ", i);
            res += &frame.to_sexp();
        }
        res += ")\n";

        res += ")\n";
        res
    }
}

impl ToSexp for ExeResult {
    fn to_sexp(&self) -> String {
        match self {
            ExeResult::Ok => "ok".to_string(),
            ExeResult::AssertErr => "\"assertion error\"".to_string(),
            ExeResult::OutOfBounds => "\"out of bounds\"".to_string(),
            ExeResult::NullPointer => "\"null pointer\"".to_string(),
            ExeResult::Div0 => "\"divide by zero\"".to_string(),
            ExeResult::NoHalt => "\"*\"".to_string(),
            ExeResult::DidNotFinish => panic!("cannot make DidNotFinish a sexp"),
        }
    }
}

impl ToSexp for Either {
    fn to_sexp(&self) -> String {
        match self {
            Either::State(state) => state.to_sexp(),
            Either::Result(exe_result) => exe_result.to_sexp(),
        }
    }
}
