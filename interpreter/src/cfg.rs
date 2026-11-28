use crate::{
    instruction::{Access, Instruction, Op},
    java_class::{Class, Method},
    java_types::SimpleRef,
};

// CFG = control flow graph
#[derive(Debug)]
pub struct MethodCFG {
    pub nodes: Vec<CfgNode>,
    //method: Method,
}

#[derive(Clone, Copy, Debug)]
pub struct CfgNode {
    pub backedges: u32,
    pub node_type: CfgNodeType,
}

#[derive(Clone, Copy, Debug)]
pub enum CfgNodeType {
    Branch(usize, usize),
    Jump(usize),
    // the return node, and the backedges in the called method
    Call(usize, u32),
    End,
}

impl MethodCFG {
    fn count_backedges(&mut self) {
        for offset in (0..self.nodes.len()).rev() {
            match self.nodes[offset].node_type {
                CfgNodeType::Branch(a, b) => {
                    if offset < a {
                        self.nodes[offset].backedges += self.nodes[a].backedges;
                    } else {
                        self.nodes[offset].backedges += 1;
                    }

                    if offset < b {
                        self.nodes[offset].backedges += self.nodes[b].backedges;
                    } else {
                        self.nodes[offset].backedges += 1;
                    }
                }
                CfgNodeType::Jump(a) => {
                    if offset < a {
                        self.nodes[offset].backedges = self.nodes[a].backedges;
                    } else {
                        self.nodes[offset].backedges = 1;
                    }
                }
                CfgNodeType::Call(a, invoked_backedges) => {
                    if offset < a {
                        self.nodes[offset].backedges = self.nodes[a].backedges + invoked_backedges;
                    } else {
                        self.nodes[offset].backedges = 1 + invoked_backedges;
                    }
                }
                CfgNodeType::End => self.nodes[offset].backedges = 0,
            }
        }
    }
}

pub type CFGsWeights = Vec<Option<Vec<u32>>>;

pub fn instruction_weights(class: &Class, method: &Method) -> Option<CFGsWeights> {
    let mut method_cfgs: Vec<_> = class
        .methods
        .iter()
        .map(|m| (m.clone(), (false, None)))
        .collect();
    let Some(m_indx) = method_cfgs.iter().position(|x| x.0.id == method.id) else {
        panic!()
    };
    fill_method_cfg(&mut method_cfgs, &class.name, m_indx);
    if method_cfgs[m_indx].1.1.is_none() {
        None
    } else {
        Some(
            method_cfgs
                .into_iter()
                .map(|(_, (_, cfg_opt))| {
                    cfg_opt.map(|cfg| cfg.nodes.into_iter().map(|node| node.backedges).collect())
                })
                .collect(),
        )
    }
}

fn fill_method_cfg(
    method_cfgs: &mut Vec<(Method, (bool, Option<MethodCFG>))>,
    current_class_name: &str,
    m_indx: usize,
) {
    let started = method_cfgs[m_indx].1.0;
    if started {
        // we do not handle mutual recursion
        method_cfgs[m_indx].1.1 = None;
        return;
    }
    method_cfgs[m_indx].1.0 = true;
    let method = method_cfgs[m_indx].0.clone();

    let mut res = MethodCFG {
        nodes: vec![
            CfgNode {
                backedges: 0,
                node_type: CfgNodeType::End
            };
            method.instructions.len() + 1
        ],
    };
    for (offset, instruction) in method.instructions.iter().enumerate() {
        match instruction {
            Instruction::Load { .. }
            | Instruction::Push { .. }
            | Instruction::Dup { .. }
            | Instruction::Store { .. }
            | Instruction::New { .. }
            | Instruction::NoOp
            | Instruction::DupX2
            | Instruction::Incr { .. }
            | Instruction::Neg { .. }
            | Instruction::Put { .. }
            | Instruction::CompareFloat { .. }
            | Instruction::CompareLong
            | Instruction::BitOpr { .. }
            | Instruction::Get { .. }
            | Instruction::Cast { .. } => {
                res.nodes[offset].node_type = CfgNodeType::Jump(offset + 1)
            }
            Instruction::Throw | Instruction::Return { .. } => {
                res.nodes[offset].node_type = CfgNodeType::Jump(res.nodes.len() - 1)
            }
            Instruction::Ifz { target, .. } => {
                res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, *target as usize)
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
                    SimpleRef::Array { ty: _ } => {
                        method_cfgs[m_indx].1.1 = None;
                        return;
                    }
                };
                let methodname = method_id.name.as_str();
                if current_class_name != *classname {
                    if ((classname == "java/lang/AssertionError"
                        || classname == "java/lang/Object")
                        && methodname == "<init>")
                        || (classname == "java/lang/Math"
                            && [
                                "abs",
                                "sqrt",
                                "min",
                                "max",
                                "floor",
                                "ceil",
                                "rint",
                                "getExponent",
                                "log",
                                "exp",
                                "pow",
                                "sin",
                                "cos",
                                "tan",
                                "atan",
                                "cosh",
                                "tanh",
                            ]
                            .contains(&methodname))
                        || (classname == "java/lang/Double"
                            && ["doubleToRawLongBits", "longBitsToDouble"].contains(&methodname))
                    {
                        res.nodes[offset].node_type = CfgNodeType::Jump(offset + 1);
                    } else {
                        eprintln!("{:?}", methodname);
                        method_cfgs[m_indx].1.1 = None;
                        return;
                    }
                } else {
                    if method.id == *method_id {
                        res.nodes[offset].node_type = CfgNodeType::Jump(0);
                    } else {
                        let Some(invoked_indx) =
                            method_cfgs.iter().position(|x| x.0.id == *method_id)
                        else {
                            panic!()
                        };
                        /*
                        dbg!(&method.id.name);
                        dbg!(invoked_indx, &self.method_cfgs);
                        */
                        let started = method_cfgs[invoked_indx].1.0;
                        if !started {
                            fill_method_cfg(method_cfgs, current_class_name, invoked_indx);
                        }
                        let Some(ref invoked_cfg) = method_cfgs[invoked_indx].1.1 else {
                            method_cfgs[m_indx].1.1 = None;
                            return;
                        };
                        let invoked_backedges = invoked_cfg.nodes[0].backedges;
                        res.nodes[offset].node_type =
                            CfgNodeType::Call(offset + 1, invoked_backedges);
                    }
                }
            }
            Instruction::Binary { op, .. } => {
                let can_div0 = matches!(op, Op::Div | Op::Rem);
                if can_div0 {
                    res.nodes[offset].node_type =
                        CfgNodeType::Branch(offset + 1, res.nodes.len() - 1);
                } else {
                    res.nodes[offset].node_type = CfgNodeType::Jump(offset + 1);
                }
            }
            Instruction::If { target, .. } => {
                res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, *target as usize)
            }
            Instruction::Goto { target } => {
                res.nodes[offset].node_type = CfgNodeType::Jump(*target as usize)
            }
            Instruction::Placeholder => todo!(),
            Instruction::NewArray { .. } => {
                res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
            } // if length is greater than LIMIT reaches DidNotFinish otherwise continues
            Instruction::ArrayStore { .. } => {
                res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
            } // if array is nullpointer or out of bounds we fail
            Instruction::ArrayLength => {
                res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
            } // if array is nullpointer we fail
            Instruction::ArrayLoad { .. } => {
                res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
            } // if array is nullpointer or out of bounds we fail
        }
    }
    res.count_backedges();
    method_cfgs[m_indx].1.1 = Some(res);
}
