use crate::{
    instruction::{Instruction, Op},
    java_class::Method,
};

#[derive(Debug)]
// control flow graph
pub struct CFG {
    pub nodes: Vec<CfgNode>,
}

#[derive(Clone, Copy, Debug)]
pub struct CfgNode {
    pub backedges: Option<u32>,
    pub node_type: CfgNodeType,
}

#[derive(Clone, Copy, Debug)]
pub enum CfgNodeType {
    Branch(usize, usize),
    Jump(usize),
    End,
}

impl CFG {
    pub fn from_instructions(instructions: &[Instruction]) -> Option<Self> {
        let mut res = Self {
            nodes: vec![
                CfgNode {
                    backedges: None,
                    node_type: CfgNodeType::End
                };
                instructions.len() + 1
            ],
        };
        for (offset, instruction) in instructions.iter().enumerate() {
            match instruction {
                Instruction::Load { .. }
                | Instruction::Push { .. }
                | Instruction::Dup { .. }
                | Instruction::Store { .. }
                | Instruction::New { .. }
                | Instruction::NoOp { .. }
                | Instruction::DupX2 { .. }
                | Instruction::Incr { .. }
                | Instruction::Neg { .. }
                | Instruction::Put { .. }
                | Instruction::CompareFloat { .. }
                | Instruction::CompareLong { .. }
                | Instruction::BitOpr { .. }
                | Instruction::Get { .. }
                | Instruction::Cast { .. } => {
                    res.nodes[offset].node_type = CfgNodeType::Jump(offset + 1)
                }
                Instruction::Throw { .. } | Instruction::Return { .. } => {
                    res.nodes[offset].node_type = CfgNodeType::Jump(res.nodes.len() - 1)
                }
                Instruction::Ifz { target, .. } => {
                    res.nodes[offset].node_type = CfgNodeType::Branch(offset + 1, *target as usize)
                }
                Instruction::Invoke { .. } => return None,
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
                Instruction::Placeholder { .. } => todo!(),
                Instruction::NewArray { .. } => {
                    res.nodes[offset].node_type =
                        CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
                } // if length is greater than LIMIT reaches DidNotFinish otherwise continues
                Instruction::ArrayStore { .. } => {
                    res.nodes[offset].node_type =
                        CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
                } // if array is nullpointer or out of bounds we fail
                Instruction::ArrayLength { .. } => {
                    res.nodes[offset].node_type =
                        CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
                } // if array is nullpointer we fail
                Instruction::ArrayLoad { .. } => {
                    res.nodes[offset].node_type =
                        CfgNodeType::Branch(offset + 1, res.nodes.len() - 1)
                } // if array is nullpointer or out of bounds we fail
            }
        }
        res.count_backedges(0);
        Some(res)
    }

    fn count_backedges(&mut self, offset: usize) {
        if self.nodes[offset].backedges != None {
            return;
        }

        match self.nodes[offset].node_type {
            CfgNodeType::Branch(a, b) => {
                let mut backedges_count = 0;
                if offset < a {
                    self.count_backedges(a);
                    let Some(a_backedges) = self.nodes[a].backedges else {
                        panic!();
                    };
                    backedges_count += a_backedges;
                } else {
                    backedges_count += 1;
                }

                if offset < b {
                    self.count_backedges(b);
                    let Some(b_backedges) = self.nodes[b].backedges else {
                        panic!();
                    };
                    backedges_count += b_backedges;
                } else {
                    backedges_count += 1;
                }

                self.nodes[offset].backedges = Some(backedges_count);
            }
            CfgNodeType::Jump(a) => {
                if offset < a {
                    self.count_backedges(a);
                    self.nodes[offset].backedges = self.nodes[a].backedges;
                } else {
                    self.nodes[offset].backedges = Some(1);
                }
            }
            CfgNodeType::End => self.nodes[offset].backedges = Some(0),
        }
    }
}
