use std::collections::HashMap;

use z3::ast::{BV, Bool};

use crate::{
    state::ExeResult::{AssertErr, DidNotFinish, Div0, NullPointer, Ok, OutOfBounds},
    *,
};

#[derive(Debug, Clone)]
pub struct Interpreter {
    class: Class,
}
impl Interpreter {
    pub fn class(&self) -> &Class {
        &self.class
    }
    pub fn new(class: Class) -> Self {
        Interpreter { class }
    }

    fn init_class(&self, mut state: State) -> State {
        let Some(clinit) = self.class.methods.iter().find(|m| m.id.name == "<clinit>") else {
            return state;
        };
        let clinit = clinit.id.clone();
        let pc = ProgramCounter {
            class: self.class.name.clone(),
            method: clinit,
            idx: 0,
        };
        let base_frame_count = state.frames.len();
        state.frames.push(Frame::new(pc, vec![]));
        loop {
            let (outcome, _) = self.step(state);

            match outcome {
                Either::State(next_state) => {
                    state = next_state;
                    if state.frames.len() == base_frame_count {
                        return state;
                    }
                }
                Either::Result(..) => {
                    panic!();
                }
            }
        }
    }

    pub fn interpret(
        &self,
        method: &Method,
        input: (Vec<StackValue>, Heap),
        iter: u32,
    ) -> (Vec<State>, Vec<z3::ast::Bool>, ExeResult) {
        let pc = ProgramCounter {
            class: self.class.name.clone(),
            method: method.id.clone(),
            idx: 0,
        };
        let (args, heap) = input;
        let mut state = State::new(pc.clone(), args, heap, &self.class.static_fields);
        state = self.init_class(state);
        if !method.is_static {
            let constructor = self
                .class
                .methods
                .iter()
                .find(|m| m.id.params.is_empty() && m.id.name == "<init>");
            if let Some(constructor) = constructor {
                state.frames.push(Frame::new(
                    ProgramCounter {
                        class: self.class.name.clone(),
                        method: constructor.id.clone(),
                        idx: 0,
                    },
                    vec![StackValue::Ref(Some(0), z3_true(), BV::from_u64(0, 32))],
                ));
            } else {
                panic!()
            }
        }
        let mut path = vec![];
        let mut states = vec![state.clone()];
        for _ in 0..iter {
            let (outcome, constraint) = self.step(state);
            if let Some(constraint) = constraint {
                path.push(constraint);
            }

            match outcome {
                Either::State(next_state) => {
                    states.push(next_state.clone());
                    state = next_state;
                }
                Either::Result(exe_result) => return (states, path, exe_result),
            }
        }
        (states, path, ExeResult::DidNotFinish)
    }

    fn step(&self, mut state: State) -> (Either, Option<z3::ast::Bool>) {
        let Some(mut cur_frame) = state.frames.pop() else {
            panic!("Empty state!")
        };
        let bytecode = self
            .class
            .methods
            .iter()
            .filter(|m| m.id == cur_frame.program_counter.method)
            .map(|m| &m.instructions)
            .next()
            .unwrap();
        // dbg!(&bytecode[cur_frame.pc()]);
        match &bytecode[cur_frame.pc()] {
            Instruction::Load { ty, index } => cur_frame.load(*ty, *index),
            Instruction::Push { value } => match value {
                Constant::String(s) => {
                    let idx = state.heap.heap.len();
                    cur_frame.push(StackValue::Ref(
                        Some(idx as u32),
                        z3_false(),
                        BV::from_u64(idx as u64, 32),
                    ));
                    state.heap.heap.push(util::create_string(s, false));
                }
                value => cur_frame.push(value.to_stack_value()),
            },
            Instruction::Dup { words } => cur_frame.dup(*words),
            Instruction::Throw => return (Either::Result(AssertErr), None),
            Instruction::Ifz { cond, target } => {
                let (real_bool, sym_bool) = match cur_frame.stack.pop() {
                    Some(StackValue::Int(i, bv)) => (
                        cond.cmp_i32(i, 0),
                        Some(cond.cmp_z3_signed(&bv, &z3::ast::BV::from_i64(0, 32))),
                    ),
                    Some(StackValue::Ref(r, sym_is_null, sym_bv)) => (
                        cond.cmp_ref(r, None),
                        Some(cond.cmp_z3_ref(
                            (&sym_is_null, &sym_bv),
                            (
                                &z3::ast::Bool::from_bool(false),
                                &z3::ast::BV::from_u64(0, 32),
                            ),
                        )),
                    ),
                    Some(_) => panic!(),
                    None => panic!(),
                };
                if real_bool {
                    cur_frame.set_pc(*target);
                    state.frames.push(cur_frame);
                    return (Either::State(state), sym_bool);
                } else {
                    cur_frame.increment_pc();
                    state.frames.push(cur_frame);
                    return (Either::State(state), sym_bool.map(|b| b.not()));
                }
            }
            Instruction::Store { ty, index } => cur_frame.store(*ty, *index),
            Instruction::Return { ty } => {
                if let Some(old_frame) = state.frames.last_mut() {
                    if let Some(ty) = ty {
                        let Some(value) = cur_frame.stack.pop() else {
                            panic!(
                                "Invalid frame {}, stack shouldn't be empty!",
                                cur_frame.to_sexp()
                            )
                        };
                        assert!(*ty == value.get_type());
                        old_frame.push(value);
                    }
                    return (Either::State(state), None);
                } else {
                    if cur_frame.program_counter.method.ret_ty.is_some() {
                        let value = cur_frame.stack.pop().expect("Stack shouldn't be empty!");
                        return (Either::Result(Ok(Some((value, state.heap)))), None);
                    } else {
                        return (Either::Result(Ok(None)), None);
                    }
                }
            }
            Instruction::Get {
                is_static,
                class_name,
                field_name,
                field_ty: _,
            } => {
                if field_name == "$assertionsDisabled" {
                    cur_frame
                        .stack
                        .push(StackValue::Int(0, z3::ast::BV::from_i64(0, 32)));
                } else if *is_static && *class_name == self.class.name {
                    cur_frame.stack.push(state.statics[field_name].clone());
                } else if *is_static {
                    todo!()
                } else if *class_name != self.class.name {
                    todo!()
                } else {
                    let StackValue::Ref(Some(r), _, _) = cur_frame.stack.pop().unwrap() else {
                        panic!()
                    };
                    let HeapValue::Object { name: _, fields } = &state.heap.heap[r as usize] else {
                        panic!()
                    };
                    let field = fields[field_name].to_stack_value();
                    cur_frame.stack.push(field);
                }
            }
            Instruction::New { class } => {
                let idx = state.heap.heap.len();
                cur_frame.stack.push(StackValue::Ref(
                    Some(idx as u32),
                    z3_true(),
                    z3::ast::BV::from_u64(idx as u64, 32),
                ));
                let obj = if *class == self.class.name {
                    self.class.new_object()
                } else {
                    HeapValue::Object {
                        name: class.clone(),
                        fields: HashMap::new(),
                    }
                };
                state.heap.heap.push(obj);
            }
            Instruction::Invoke {
                access,
                method_id,
                simple_ref,
            } => {
                assert!(matches!(
                    access,
                    Access::Static | Access::Special | Access::Virtual
                ));
                // eprintln!("{}", method_id.name);
                // If this is not a method of the class, it's one of the special cases
                // Or it's unhandled and we panic
                let classname = match simple_ref {
                    SimpleRef::Class { name } => name,
                    SimpleRef::Array { ty: _ } => todo!(),
                };
                if self.class.name != *classname {
                    if (classname == "java/lang/AssertionError" || classname == "java/lang/Object")
                        && method_id.name == "<init>"
                    {
                        cur_frame.stack.pop();
                    } else if classname == "java/lang/Math" {
                        match method_id.name.as_str() {
                            "abs" => {
                                let val = cur_frame.stack.pop().unwrap();
                                let abs = util::abs(&val);
                                cur_frame.stack.push(abs);
                            }
                            "sqrt" => {
                                let StackValue::Double(val, sym) = cur_frame.stack.pop().unwrap()
                                else {
                                    panic!()
                                };
                                let sqrt = val.sqrt();
                                let sym_sqrt = sym.sqrt();
                                cur_frame.stack.push(StackValue::Double(sqrt, sym_sqrt));
                            }
                            "min" => {
                                let rhs = cur_frame.stack.pop().unwrap();
                                let lhs = cur_frame.stack.pop().unwrap();
                                cur_frame.stack.push(lhs.min(&rhs));
                            }
                            "max" => {
                                let rhs = cur_frame.stack.pop().unwrap();
                                let lhs = cur_frame.stack.pop().unwrap();
                                cur_frame.stack.push(lhs.max(&rhs));
                            }
                            "floor" | "ceil" | "rint" => {
                                use z3::ast::RoundingMode;
                                let StackValue::Double(val, sym) = cur_frame.stack.pop().unwrap()
                                else {
                                    panic!("Math.{} expects a double", method_id.name)
                                };
                                let (res, rm) = match method_id.name.as_str() {
                                    "floor" => {
                                        (val.floor(), RoundingMode::round_towards_negative())
                                    }
                                    "ceil" => (val.ceil(), RoundingMode::round_towards_positive()),
                                    _ => (
                                        val.round_ties_even(),
                                        RoundingMode::round_nearest_ties_to_even(),
                                    ),
                                };
                                cur_frame.stack.push(StackValue::Double(
                                    res,
                                    sym.round_to_integral_with_rounding_mode(&rm),
                                ));
                            }
                            "getExponent" => {
                                let val = cur_frame.stack.pop().unwrap();
                                cur_frame.stack.push(util::math_get_exponent(&val));
                            }
                            name => {
                                let concrete: fn(&[f64]) -> f64 = match name {
                                    "log" => |a| a[0].ln(),
                                    "exp" => |a| a[0].exp(),
                                    "pow" => |a| a[0].powf(a[1]),
                                    "sin" => |a| a[0].sin(),
                                    "cos" => |a| a[0].cos(),
                                    "tan" => |a| a[0].tan(),
                                    "atan" => |a| a[0].atan(),
                                    "cosh" => |a| a[0].cosh(),
                                    "tanh" => |a| a[0].tanh(),
                                    _ => todo!("java/lang/Math.{name}"),
                                };
                                let n = method_id.params.len();
                                let args = cur_frame.stack.split_off(cur_frame.stack.len() - n);
                                cur_frame.stack.push(util::math_uf(name, &args, concrete));
                            }
                        }
                    } else if classname == "java/lang/Double" {
                        match method_id.name.as_str() {
                            "doubleToRawLongBits" => {
                                let StackValue::Double(val, sym) = cur_frame.stack.pop().unwrap()
                                else {
                                    panic!("doubleToRawLongBits expects a double")
                                };
                                cur_frame
                                    .stack
                                    .push(StackValue::Long(val.to_bits() as i64, sym.to_ieee_bv()));
                            }
                            "longBitsToDouble" => {
                                let StackValue::Long(val, bv) = cur_frame.stack.pop().unwrap()
                                else {
                                    panic!("longBitsToDouble expects a long")
                                };
                                cur_frame.stack.push(StackValue::Double(
                                    f64::from_bits(val as u64),
                                    util::z3_ieee_bv_to_float(&bv, &z3::Sort::double()),
                                ));
                            }
                            _ => todo!(
                                "Handling {} method on class {} not yet implemented",
                                method_id.name,
                                classname
                            ),
                        }
                    } else {
                        todo!(
                            "Handling {} method on class {} not yet implemented",
                            method_id.name,
                            classname
                        );
                    }
                } else {
                    let new_pc = ProgramCounter {
                        class: self.class.name.clone(),
                        method: method_id.clone(),
                        idx: 0,
                    };
                    // Non-static calls also pop the receiver, which becomes local 0
                    let n_args =
                        method_id.params.len() + usize::from(!matches!(access, Access::Static));
                    let new_locals = cur_frame
                        .stack
                        .drain(cur_frame.stack.len() - n_args..)
                        .collect();
                    let new_frame = Frame::new(new_pc, new_locals);
                    cur_frame.increment_pc();
                    state.frames.push(cur_frame);
                    state.frames.push(new_frame);
                    return (Either::State(state), None);
                }
            }
            Instruction::Binary { op, ty: _ } => {
                let rhs = cur_frame.stack.pop().unwrap();
                let lhs = cur_frame.stack.pop().unwrap();
                let (op_res, is_rhs_zero) = match (lhs, rhs) {
                    (StackValue::Int(lhs, lhs_sym), StackValue::Int(rhs, rhs_sym)) => (
                        op.op_int(lhs, rhs)
                            .map(|i| StackValue::Int(i, op.op_z3_bv(&lhs_sym, &rhs_sym))),
                        rhs_sym.eq(0),
                    ),
                    (StackValue::Float(lhs, lhs_sym), StackValue::Float(rhs, rhs_sym)) => (
                        Some(StackValue::Float(
                            op.op_f32(lhs, rhs),
                            op.op_z3_float(&lhs_sym, &rhs_sym),
                        )),
                        z3_false(),
                    ),
                    (StackValue::Long(lhs, lhs_sym), StackValue::Long(rhs, rhs_sym)) => (
                        op.op_long(lhs, rhs)
                            .map(|i| StackValue::Long(i, op.op_z3_bv(&lhs_sym, &rhs_sym))),
                        rhs_sym.eq(0),
                    ),
                    (StackValue::Double(lhs, lhs_sym), StackValue::Double(rhs, rhs_sym)) => (
                        Some(StackValue::Double(
                            op.op_f64(lhs, rhs),
                            op.op_z3_float(&lhs_sym, &rhs_sym),
                        )),
                        z3_false(),
                    ),
                    _ => panic!("Invalid types"),
                };
                // Only add contraint if division can fail
                let can_div0 = matches!(op, Op::Div | Op::Rem);
                match op_res {
                    Some(result) => {
                        cur_frame.stack.push(result);
                        cur_frame.increment_pc();
                        state.frames.push(cur_frame);
                        return (Either::State(state), can_div0.then(|| is_rhs_zero.not()));
                    }
                    None => return (Either::Result(Div0), Some(is_rhs_zero)),
                }
            }
            Instruction::If { cond, target } => {
                let rhs = cur_frame.stack.pop().unwrap();
                let lhs = cur_frame.stack.pop().unwrap();
                let (real_bool, sym_bool) = match (lhs, rhs) {
                    (StackValue::Int(lhs, lhs_bv), StackValue::Int(rhs, rhs_bv)) => (
                        cond.cmp_i32(lhs, rhs),
                        Some(cond.cmp_z3_signed(&lhs_bv, &rhs_bv)),
                    ),
                    (
                        StackValue::Ref(lhs, lhs_is_null, lhs_bv),
                        StackValue::Ref(rhs, rhs_is_null, rhs_bv),
                    ) => (
                        cond.cmp_ref(lhs, rhs),
                        Some(cond.cmp_z3_ref((&lhs_is_null, &lhs_bv), (&rhs_is_null, &rhs_bv))),
                    ),
                    _ => panic!("Cannot compare these types"),
                };

                if real_bool {
                    cur_frame.set_pc(*target);
                    state.frames.push(cur_frame);
                    return (Either::State(state), sym_bool);
                } else {
                    cur_frame.increment_pc();
                    state.frames.push(cur_frame);
                    return (Either::State(state), sym_bool.map(|b| b.not()));
                }
            }
            Instruction::Goto { target } => {
                cur_frame.set_pc(*target);
                state.frames.push(cur_frame);
                return (Either::State(state), None);
            }
            Instruction::Placeholder => todo!(),
            Instruction::NewArray { dim, ty } => {
                // length is the product of dimensions
                // array elements are accessed by only one index so dimensions don't need to be saved?
                // let arr_length = (0..dim).map(|| cur_frame.stack.pop()).product();

                if *dim != 1 {
                    todo!("Only one dimensional arrays are supported currently!");
                }

                let len = cur_frame.stack.pop().unwrap();
                let StackValue::Int(l, bv) = &len else {
                    panic!("Invalid stackvalue, should be Int!")
                };
                const LIMIT: i32 = 10000;
                let sym_gt_limit = bv.bvsgt(LIMIT);
                if *l > LIMIT {
                    return (Either::Result(DidNotFinish), Some(sym_gt_limit));
                }

                let idx = state.heap.heap.len();
                cur_frame.stack.push(StackValue::Ref(
                    Some(idx as u32),
                    z3_true(),
                    z3::ast::BV::from_u64(idx as u64, 32),
                ));
                state
                    .heap
                    .heap
                    .push(HeapValue::Array(Array::new_zero(*ty, len)));

                cur_frame.increment_pc();
                state.frames.push(cur_frame);
                return (Either::State(state), Some(sym_gt_limit.not()));
            }
            Instruction::ArrayStore { ty: _ } => {
                let val = cur_frame
                    .stack
                    .pop()
                    .expect("[ArraySotre]: value not on stack");
                let StackValue::Int(idx, idx_bv) = cur_frame
                    .stack
                    .pop()
                    .expect("[ArraySotre]: index not on stack")
                else {
                    panic!("Expected an Int");
                };

                let StackValue::Ref(arr_ref, sym_is_valid, _) = cur_frame
                    .stack
                    .pop()
                    .expect("[ArraySotre]: arrayref not on stack")
                else {
                    panic!("Expected a Ref");
                };
                let is_null_pointer = sym_is_valid.eq(z3_false());
                let Some(arr) = arr_ref else {
                    return (Either::Result(NullPointer), Some(is_null_pointer));
                };

                let HeapValue::Array(Array {
                    ty,
                    values,
                    symbolic,
                    len,
                }) = &mut state.heap.heap[arr as usize]
                else {
                    panic!("Not array ref");
                };
                let is_out_of_bounds = z3::ast::Bool::or(&[idx_bv.bvslt(0), idx_bv.bvsge(&*len)]);
                // TODO: check type
                if idx as usize >= values.len() {
                    return (
                        Either::Result(OutOfBounds),
                        Some(z3::ast::Bool::and(&[
                            is_null_pointer.not(),
                            is_out_of_bounds,
                        ])),
                    );
                }

                let array_val = val.to_array_value(*ty);
                *symbolic = symbolic.store(&idx_bv, &array_val.z3());
                values[idx as usize] = array_val;

                cur_frame.increment_pc();
                state.frames.push(cur_frame);
                return (
                    Either::State(state),
                    Some(z3::ast::Bool::and(&[
                        is_null_pointer.not(),
                        is_out_of_bounds.not(),
                    ])),
                );
            }
            Instruction::ArrayLength => {
                let StackValue::Ref(arr_ref, sym_is_valid, _) = cur_frame
                    .stack
                    .pop()
                    .expect("[ArraySotre]: arrayref not on stack")
                else {
                    panic!("Expected a Ref");
                };

                let Some(arr) = arr_ref else {
                    return (
                        Either::Result(NullPointer),
                        Some(sym_is_valid.eq(z3_false())),
                    );
                };

                if let HeapValue::Array(Array { values, len, .. }) = &state.heap.heap[arr as usize]
                {
                    cur_frame.push(StackValue::Int(values.len() as i32, len.clone()));
                } else {
                    panic!("Not array ref");
                }

                cur_frame.increment_pc();
                state.frames.push(cur_frame);
                return (Either::State(state), Some(sym_is_valid.eq(z3_true())));
            }
            Instruction::ArrayLoad { ty: _ } => {
                let StackValue::Int(idx, idx_bv) = cur_frame
                    .stack
                    .pop()
                    .expect("[ArrayLoad]: index not on stack")
                else {
                    panic!("Expected an Int");
                };

                let StackValue::Ref(arr_ref, sym_is_valid, _) = cur_frame
                    .stack
                    .pop()
                    .expect("[ArraySotre]: arrayref not on stack")
                else {
                    panic!("Expected a Ref");
                };

                let is_null_pointer = sym_is_valid.eq(z3_false());
                let Some(arr) = arr_ref else {
                    return (Either::Result(NullPointer), Some(is_null_pointer));
                };

                let HeapValue::Array(Array {
                    ty: _,
                    values,
                    symbolic,
                    len,
                }) = &mut state.heap.heap[arr as usize]
                else {
                    panic!("Not array ref");
                };
                let is_out_of_bounds = z3::ast::Bool::or(&[idx_bv.bvslt(0), idx_bv.bvsge(&*len)]);

                if idx as usize >= values.len() {
                    return (
                        Either::Result(OutOfBounds),
                        Some(z3::ast::Bool::and(&[
                            is_null_pointer.not(),
                            is_out_of_bounds,
                        ])),
                    );
                }
                let sym_val = symbolic.select(&idx_bv);
                cur_frame.push(values[idx as usize].swap_sym_val(sym_val).to_stack_value());
                cur_frame.increment_pc();
                state.frames.push(cur_frame);
                return (
                    Either::State(state),
                    Some(z3::ast::Bool::and(&[
                        is_null_pointer.not(),
                        is_out_of_bounds.not(),
                    ])),
                );
            }
            Instruction::Incr { index, amount } => {
                let StackValue::Int(local, bv) = cur_frame.locals[*index as usize].clone().unwrap()
                else {
                    panic!("Local must be an int, local: {}", index);
                };
                cur_frame.locals[*index as usize] = Some(StackValue::Int(
                    local.wrapping_add(*amount),
                    bv.bvadd(*amount),
                ));
            }
            Instruction::Neg { ty } => {
                let value = cur_frame.stack.pop().unwrap();
                assert!(*ty == value.get_type());
                let res = match value {
                    StackValue::Int(v, bv) => StackValue::Int(v.wrapping_neg(), bv.bvneg()),
                    StackValue::Float(v, sym) => StackValue::Float(-v, sym.unary_neg()),
                    StackValue::Long(l, bv) => StackValue::Long(l.wrapping_neg(), bv.bvneg()),
                    StackValue::Double(d, sym) => StackValue::Double(-d, sym.unary_neg()),
                    StackValue::Ref(_, _, _) => panic!(),
                };
                cur_frame.push(res);
            }
            Instruction::NoOp => {}
            Instruction::Put {
                is_static,
                class_name,
                field_name,
                field_ty,
            } => {
                if *is_static && *class_name == self.class.name {
                    let field = state.statics.get_mut(field_name).unwrap();
                    *field = cur_frame.stack.pop().unwrap();
                } else if *is_static {
                    todo!()
                } else if *class_name != self.class.name {
                    todo!()
                } else {
                    let value = cur_frame.stack.pop().unwrap();
                    let StackValue::Ref(Some(r), _, _) = cur_frame.stack.pop().unwrap() else {
                        panic!()
                    };
                    let HeapValue::Object { name: _, fields } = &mut state.heap.heap[r as usize]
                    else {
                        panic!()
                    };
                    fields.insert(field_name.clone(), value.to_heap_value(field_ty));
                }
            }
            Instruction::CompareFloat { on_nan, double } => {
                let (res, sym_val1, sym_val2) = if *double {
                    let StackValue::Double(val2, sym_val2) = cur_frame.stack.pop().unwrap() else {
                        panic!()
                    };
                    let StackValue::Double(val1, sym_val1) = cur_frame.stack.pop().unwrap() else {
                        panic!()
                    };
                    let res = if val1.is_nan() || val2.is_nan() {
                        *on_nan
                    } else if val1 == val2 {
                        0
                    } else if val1 > val2 {
                        1
                    } else {
                        -1
                    };
                    (res, sym_val1, sym_val2)
                } else {
                    let StackValue::Float(val2, sym_val2) = cur_frame.stack.pop().unwrap() else {
                        panic!()
                    };
                    let StackValue::Float(val1, sym_val1) = cur_frame.stack.pop().unwrap() else {
                        panic!()
                    };

                    let res = if val1.is_nan() || val2.is_nan() {
                        *on_nan
                    } else if val1 == val2 {
                        0
                    } else if val1 > val2 {
                        1
                    } else {
                        -1
                    };
                    (res, sym_val1, sym_val2)
                };
                let zero = BV::from_i64(0, 32);
                let one = BV::from_i64(1, 32);
                let m_one = BV::from_i64(-1, 32);
                let sym_onnan = BV::from_i64(*on_nan as i64, 32);
                let a = sym_val1.gt(&sym_val2).ite(&one, &zero);
                let b = sym_val1.lt(&sym_val2).ite(&m_one, &a);
                let c = Bool::or(&[sym_val1.is_nan(), sym_val2.is_nan()]).ite(&sym_onnan, &b);
                cur_frame.stack.push(StackValue::Int(res, c));
            }
            Instruction::CompareLong => {
                let StackValue::Long(val2, sym_val2) = cur_frame.stack.pop().unwrap() else {
                    panic!()
                };
                let StackValue::Long(val1, sym_val1) = cur_frame.stack.pop().unwrap() else {
                    panic!()
                };
                let zero = BV::from_i64(0, 32);
                let one = BV::from_i64(1, 32);
                let m_one = BV::from_i64(-1, 32);
                let zero_or_one = sym_val1.bvsgt(&sym_val2).ite(&one, &zero);
                let sym_res = sym_val1.bvslt(sym_val2).ite(&m_one, &zero_or_one);
                let res = if val1 == val2 {
                    0
                } else if val1 > val2 {
                    1
                } else {
                    -1
                };
                cur_frame.stack.push(StackValue::Int(res, sym_res));
            }
            Instruction::DupX2 => {
                if cur_frame.stack[cur_frame.stack.len() - 2].is_wide() {
                    let to_dup = cur_frame.stack.pop().unwrap();
                    let middle = cur_frame.stack.pop().unwrap();
                    cur_frame.stack.push(to_dup.clone());
                    cur_frame.stack.push(middle);
                    cur_frame.stack.push(to_dup);
                } else {
                    let to_dup = cur_frame.stack.pop().unwrap();
                    let middle1 = cur_frame.stack.pop().unwrap();
                    let middle2 = cur_frame.stack.pop().unwrap();
                    cur_frame.stack.push(to_dup.clone());
                    cur_frame.stack.push(middle2);
                    cur_frame.stack.push(middle1);
                    cur_frame.stack.push(to_dup);
                }
            }
            Instruction::Cast { from, to } => {
                let value = cur_frame.stack.pop().unwrap();
                assert!(*from == value.get_type());
                cur_frame.stack.push(value.cast(to));
            }
            Instruction::BitOpr { op } => {
                let rhs = cur_frame.stack.pop().unwrap();
                let lhs = cur_frame.stack.pop().unwrap();
                let res = match (lhs, rhs) {
                    (StackValue::Int(lhs, lhs_sym), StackValue::Int(rhs, rhs_sym)) => {
                        StackValue::Int(op.op_int(lhs, rhs), op.op_z3_bv(&lhs_sym, &rhs_sym))
                    }

                    (StackValue::Long(lhs, lhs_sym), StackValue::Long(rhs, rhs_sym)) => {
                        StackValue::Long(op.op_long(lhs, rhs), op.op_z3_bv(&lhs_sym, &rhs_sym))
                    }
                    (StackValue::Long(lhs, lhs_sym), StackValue::Int(rhs, rhs_sym)) => {
                        StackValue::Long(op.op_long_int(lhs, rhs), op.op_z3_bv(&lhs_sym, &rhs_sym))
                    }
                    _ => panic!("Invalid types"),
                };
                cur_frame.stack.push(res);
            }
        }
        cur_frame.increment_pc();
        state.frames.push(cur_frame);
        (Either::State(state), None)
    }
}
