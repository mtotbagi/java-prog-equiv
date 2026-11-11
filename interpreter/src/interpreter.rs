use std::collections::HashMap;

use crate::{
    state::ExeResult::{AssertErr, Div0, NullPointer, Ok, OutOfBounds},
    *,
};
pub struct Interpreter {
    class: Class,
}
impl Interpreter {
    pub fn new(class: Class) -> Self {
        Interpreter { class }
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
        let mut state = State::new(pc.clone(), input.0, input.1);
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
            .filter(|m| m.id.name == cur_frame.program_counter.method.name)
            .map(|m| &m.instructions)
            .next()
            .unwrap();
        dbg!(&bytecode[cur_frame.pc()]);
        match &bytecode[cur_frame.pc()] {
            Instruction::Load { ty, index } => cur_frame.load(*ty, *index),
            Instruction::Push { value } => cur_frame.push(value.clone()),
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
                        return (Either::Result(Ok(Some(value))), None);
                    } else {
                        return (Either::Result(Ok(None)), None);
                    }
                }
            }
            Instruction::Get => {
                // TODO handle other cases than assertionsDisabled = false
                cur_frame
                    .stack
                    .push(StackValue::Int(0, z3::ast::BV::from_i64(0, 32)));
                dbg!(&cur_frame.stack);
            }
            Instruction::New { class } => {
                let idx = state.heap.heap.len();
                cur_frame.stack.push(StackValue::Ref(
                    Some(idx as u32),
                    z3_true(),
                    z3::ast::BV::from_u64(idx as u64, 32),
                ));
                state.heap.heap.push(HeapValue::Object {
                    name: class.clone(),
                    fields: HashMap::new(),
                });
            }
            Instruction::Invoke {
                access: _,
                method_id,
                simple_ref,
            } => {
                eprintln!("{}", method_id.name);
                // If this is not a method of the class, it's one of the special cases
                // Or it's unhandled and we panic
                let classname = match simple_ref {
                    SimpleRef::Class { name } => name,
                    SimpleRef::Array { ty: _ } => todo!(),
                };
                if self.class.name != *classname {
                    if classname == "java/lang/AssertionError" && method_id.name == "<init>" {
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
                    let new_locals = cur_frame
                        .stack
                        .drain(
                            cur_frame.stack.len() - method_id.params.len()..cur_frame.stack.len(),
                        )
                        .collect();
                    let new_frame = Frame::new(new_pc, new_locals);
                    cur_frame.increment_pc();
                    state.frames.push(cur_frame);
                    state.frames.push(new_frame);
                    return (Either::State(state), None);
                }
            }
            Instruction::Binary { op, ty } => match ty {
                StackType::Int => {
                    let Some(StackValue::Int(rhs, rhs_bv)) = cur_frame.stack.pop() else {
                        panic!()
                    };
                    let Some(StackValue::Int(lhs, lhs_bv)) = cur_frame.stack.pop() else {
                        panic!()
                    };
                    dbg!(lhs, rhs);
                    // Only add contraint if division can fail
                    let can_div0 = matches!(op, Op::Div | Op::Rem);
                    match op.op_int(lhs, rhs) {
                        Some(result) => {
                            cur_frame
                                .stack
                                .push(StackValue::Int(result, op.op_z3(&lhs_bv, &rhs_bv)));
                            cur_frame.increment_pc();
                            state.frames.push(cur_frame);
                            return (Either::State(state), can_div0.then(|| rhs_bv.ne(0)));
                        }
                        None => return (Either::Result(Div0), Some(rhs_bv.eq(0))),
                    }
                }
                StackType::Float => todo!(),
                StackType::Ref => panic!("Cannot use ref for arithmetic operations!"),
            },
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
                    (StackValue::Float(_), StackValue::Float(_)) => todo!(),
                    _ => panic!("Cannot compare different types"),
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
                let Some(value) = cur_frame.stack.pop() else {
                    panic!()
                };
                assert!(*ty == value.get_type());
                let res = match value {
                    StackValue::Int(v, bv) => StackValue::Int(v.wrapping_neg(), bv.bvneg()),
                    StackValue::Float(v) => StackValue::Float(-v),
                    StackValue::Ref(_, _, _) => panic!(),
                };
                cur_frame.push(res);
            }
            Instruction::NoOp => {}
        }
        cur_frame.increment_pc();
        state.frames.push(cur_frame);
        (Either::State(state), None)
    }
}
