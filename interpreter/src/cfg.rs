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
pub enum CfgNode {
    Branch(usize, usize),
    Jump(usize),
    End
}

impl CFG {
    pub fn from_instructions(instructions: &[Instruction]) -> Option<Self> {
        let mut res = Self {
            nodes: vec![CfgNode::End; instructions.len() + 1],
        };
        for (index, instruction) in instructions.iter().enumerate() {
            match instruction {
                Instruction::Load {..} | Instruction::Push {..} | Instruction::Dup {..} | Instruction::Store {..} | Instruction::New {..} | Instruction::NoOp {..} | Instruction::DupX2 {..} | Instruction::Incr {..} | Instruction::Neg {..} | Instruction::Put {..} | Instruction::CompareFloat {..} | Instruction::CompareLong {..} | Instruction::BitOpr {..} | Instruction::Get {..} | Instruction::Cast { .. } => res.nodes[index] = CfgNode::Jump(index+1),
                Instruction::Throw {..} | Instruction::Return {..} => res.nodes[index] = CfgNode::Jump(res.nodes.len()-1),
                Instruction::Ifz { target, .. } => res.nodes[index] = CfgNode::Branch(index+1, *target as usize),
                Instruction::Invoke {..} => return None,
                Instruction::Binary { op, .. } => {
                    let can_div0 = matches!(op, Op::Div | Op::Rem);
                    if can_div0 {
                        res.nodes[index] = CfgNode::Branch(index+1, res.nodes.len()-1);
                    }
                    else {
                        res.nodes[index] = CfgNode::Jump(index+1);
                    }
                },
                Instruction::If { target, .. } => res.nodes[index] = CfgNode::Branch(index+1, *target as usize),
                Instruction::Goto { target } => res.nodes[index] = CfgNode::Jump(*target as usize),
                Instruction::Placeholder {..} => todo!(),
                Instruction::NewArray {..} => res.nodes[index] = CfgNode::Branch(index+1, res.nodes.len()-1), // if length is greater than LIMIT reaches DidNotFinish otherwise continues
                Instruction::ArrayStore {..} => res.nodes[index] = CfgNode::Branch(index+1, res.nodes.len()-1), // if array is nullpointer or out of bounds we fail
                Instruction::ArrayLength {..} => res.nodes[index] = CfgNode::Branch(index+1, res.nodes.len()-1), // if array is nullpointer we fail
                Instruction::ArrayLoad {..} => res.nodes[index] = CfgNode::Branch(index+1, res.nodes.len()-1), // if array is nullpointer or out of bounds we fail
            }
        }
        Some(res)
    }
}
