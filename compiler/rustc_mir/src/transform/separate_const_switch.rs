use crate::transform::MirPass;
use rustc_middle::mir::*;
use rustc_middle::ty::TyCtxt;
use smallvec::SmallVec;

pub struct SeparateConstSwitch;

impl<'tcx> MirPass<'tcx> for SeparateConstSwitch {
    fn run_pass(&self, _tcx: TyCtxt<'tcx>, body: &mut Body<'tcx>) {
        separate_const_switch(body);
    }
}

pub fn separate_const_switch<'tcx>(body: &mut Body<'tcx>) {
    let mut new_edges: SmallVec<[(BasicBlock, BasicBlock); 8]> = SmallVec::new();
    let predecessors = body.predecessors();
    'block_iter: for (block_id, block) in body.basic_blocks().iter_enumerated() {
        info!("analyzing {:?}", block_id);
        if let TerminatorKind::SwitchInt {
            discr: Operand::Copy(switch_place) | Operand::Move(switch_place),
            ..
        } = block.terminator().kind
        {
            info!("found one, switch on {:?}", switch_place);
            // if the block has fewer than 2 predecessors, ignore it
            // we could maybe chain blocks that have exactly one
            // predecessor, but for now we ignore that
            if predecessors[block_id].len() < 2 {
                continue 'block_iter;
            }

            info!("promising");

            // first, let's find a non-const place
            // that determines the result of the switch
            if let Some(switch_place) = find_determining_place(switch_place, block) {
                info!("very promising, thought {:?}", switch_place);
                // we now have an input place for which it would
                // be interesting if predecessors assigned it from a const
                'predec_iter: for predecessor_id in predecessors[block_id].iter().copied() {
                    if let Some(predecessor) = body.basic_blocks().get(predecessor_id) {
                        // first we make sure the predecessor jumps
                        // in a reasonable way
                        match &predecessor.terminator().kind {
                        // the following terminators are
                        // unconditionally valid
                        TerminatorKind::Goto { .. } | TerminatorKind::SwitchInt { .. } => {}

                        // the following terminators could
                        // maybe be allowed, but they are
                        // not supported yet
                        TerminatorKind::Yield {..}

                        // the following terminators are not allowed
                        | TerminatorKind::Resume
                        | TerminatorKind::Abort
                        | TerminatorKind::Return
                        | TerminatorKind::Unreachable 
                        | TerminatorKind::InlineAsm { .. }
                        | TerminatorKind::GeneratorDrop => {
                            continue 'predec_iter;
                        }

                        TerminatorKind::Drop { place, target, .. } => {
                            if *place == switch_place || *target != block_id {
                                continue 'predec_iter;
                            }
                        }

                        TerminatorKind::DropAndReplace { place, value, target, .. } => {
                            if *target != block_id {
                                continue 'predec_iter;
                            }
                            if *place == switch_place {
                                if let Operand::Constant(_) = value {
                                    new_edges.push((predecessor_id, block_id));
                                }
                                continue 'predec_iter;
                            }
                        }

                        TerminatorKind::Call { destination, .. } => {
                            if let Some((place, target)) = destination {
                                if *place == switch_place || *target != block_id {
                                    continue 'predec_iter;
                                }
                            }
                        }

                        TerminatorKind::Assert { target, .. } => {
                            if *target != block_id {
                                continue 'predec_iter;
                            }
                        }

                        TerminatorKind::FalseEdge { real_target, .. } 
                        | TerminatorKind::FalseUnwind { real_target, .. }
                        => {
                            if *real_target != block_id {
                                continue 'predec_iter;
                            }
                        }
                    }
                    info!("super promising! final place: {:?}", switch_place);
                        if is_likely_const(switch_place, predecessor) {
                            info!("yep, found {:?} to {:?}", predecessor_id, block_id);
                            new_edges.push((predecessor_id, block_id));
                        }
                    }
                }
            }
        }
    }

    let blocks = body.basic_blocks_mut();
    if new_edges.len() > 0 {
        info!("found some! {}", new_edges.len());
    }
    for (pred_id, target_id) in new_edges {
        if let Some(new_block) = blocks.get(target_id).cloned() {
            let new_block_id = blocks.push(new_block);
            if let Some(terminator) = blocks.get_mut(pred_id).map(|x| x.terminator_mut()) {
                match terminator.kind {
                    TerminatorKind::Goto { ref mut target } => {
                        if *target == target_id {
                            *target = new_block_id
                        }
                    }
                    TerminatorKind::SwitchInt { ref mut targets, .. } => {
                        targets.all_targets_mut().iter_mut().for_each(|x| {
                            if *x == target_id {
                                *x = new_block_id
                            }
                        });
                    }
                    TerminatorKind::FalseEdge { ref mut real_target, .. } => {
                        if *real_target == target_id {
                            *real_target = new_block_id
                        }
                    }
                    TerminatorKind::Call { ref mut destination, .. } => {
                        if let Some((_, target)) = destination {
                            if *target == target_id {
                                *target = new_block_id
                            }
                        }
                    }
                    TerminatorKind::Assert { ref mut target, .. } => {
                        if *target == target_id {
                            *target = new_block_id
                        }
                    }
                    TerminatorKind::DropAndReplace { ref mut target, .. } => {
                        if *target == target_id {
                            *target = new_block_id
                        }
                    }
                    TerminatorKind::Drop { ref mut target, .. } => {
                        if *target == target_id {
                            *target = new_block_id
                        }
                    }
                    TerminatorKind::FalseUnwind { ref mut real_target, .. } => {
                        if *real_target == target_id {
                            *real_target = new_block_id
                        }
                    }
                    TerminatorKind::Resume
                    | TerminatorKind::Abort
                    | TerminatorKind::Return
                    | TerminatorKind::Unreachable
                    | TerminatorKind::GeneratorDrop
                    | TerminatorKind::InlineAsm { .. }
                    | TerminatorKind::Yield { .. } => (),
                };
            }
        }
    }
}

/// This function describes a rough heuristic guessing
/// whether a place is last set with a const within the block.
/// Notably, it will be overly pessimist in cases that are already
/// not handled by `separate_const_switch`.
fn is_likely_const<'tcx>(mut tracked_place: Place<'tcx>, block: &BasicBlockData<'tcx>) -> bool {
    for statement in block.statements.iter().rev() {
        info!("likely const: {:?}, current track: {:?}", statement, tracked_place);
        match &statement.kind {
            StatementKind::Assign(assign) => {
                if assign.0 == tracked_place {
                    match assign.1 {
                        Rvalue::Use(Operand::Constant(_)) => return true,
                        Rvalue::Use(Operand::Copy(place) | Operand::Move(place)) => {
                            tracked_place = place
                        }
                        Rvalue::Repeat(_, _) => return false,
                        Rvalue::Ref(_, _, _) => return true,
                        Rvalue::ThreadLocalRef(_) => return false,
                        Rvalue::AddressOf(_, _) => return true,
                        Rvalue::Len(_) => return false,
                        Rvalue::Cast(_, Operand::Copy(place) | Operand::Move(place), _) => {
                            tracked_place = place
                        }
                        Rvalue::Cast(_, Operand::Constant(_), _) => return true,
                        Rvalue::BinaryOp(_, _) => return false,
                        Rvalue::CheckedBinaryOp(_, _) => return false,
                        Rvalue::NullaryOp(_, _) => return true,
                        Rvalue::UnaryOp(_, Operand::Copy(place) | Operand::Move(place)) => {
                            tracked_place = place
                        }
                        Rvalue::UnaryOp(_, Operand::Constant(_)) => return true,
                        Rvalue::Discriminant(place) => tracked_place = place,
                        Rvalue::Aggregate(_, _) => return false,
                    }
                }
            }
            StatementKind::SetDiscriminant { place, .. } => {
                info!("found setdisc, with place {:?}, wanted {:?}", place, tracked_place);
                if **place == tracked_place {
                    return true;
                }
            }
            StatementKind::FakeRead(_)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_) => {}
            StatementKind::LlvmInlineAsm(_) => return false,
            StatementKind::Retag(_, _) => {}
            StatementKind::AscribeUserType(_, _) => {}
            StatementKind::Coverage(_) => {}
            StatementKind::CopyNonOverlapping(_) => return false,
            StatementKind::Nop => {}
        }
    }

    false
}

fn find_determining_place<'tcx>(
    mut switch_place: Place<'tcx>,
    block: &BasicBlockData<'tcx>,
) -> Option<Place<'tcx>> {
    let mut tracking_only_discriminant = false;
    for statement in block.statements.iter().rev() {
        info!("Going through {:?}", statement);
        match &statement.kind {
            StatementKind::Assign(op) => {
                if op.0 != switch_place || tracking_only_discriminant {
                    continue;
                }

                match op.1 {
                    // The following rvalues move the place
                    // that may be const in the predecessor
                    Rvalue::Use(Operand::Move(new) | Operand::Copy(new))
                    | Rvalue::UnaryOp(_, Operand::Copy(new) | Operand::Move(new))
                    | Rvalue::Cast(_, Operand::Move(new) | Operand::Copy(new), _)
                    => {
                        switch_place = new;
                    }

                    Rvalue::Discriminant(new) => {
                        switch_place = new;
                        tracking_only_discriminant = true;
                    }

                    // The following rvalues might still make the block
                    // be valid but for now we reject them
                    Rvalue::Len(_)
                    | Rvalue::Ref(_, _, _)
                    | Rvalue::BinaryOp(_, _)
                    | Rvalue::CheckedBinaryOp(_, _)

                    // The following rvalues definitely mean we cannot
                    // or should not apply this optimization
                    | Rvalue::Use(Operand::Constant(_))
                    | Rvalue::Repeat(_, _)
                    | Rvalue::ThreadLocalRef(_)
                    | Rvalue::AddressOf(_, _)
                    | Rvalue::NullaryOp(_, _)
                    | Rvalue::UnaryOp(_, Operand::Constant(_))
                    | Rvalue::Cast(_, Operand::Constant(_), _)
                    | Rvalue::Aggregate(_, _)
                    => {
                        return None;
                    }
                }
            }

            // these statements have no influence on the place
            // we are interested in
            StatementKind::FakeRead(_)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Retag(_, _)
            | StatementKind::AscribeUserType(_, _)
            | StatementKind::Coverage(_)
            | StatementKind::Nop => {}

            // these statements definitely mean we cannot
            // or should not apply this optimization
            // CopyNonOverlapping might still be
            // usable, but for now we reject it
            StatementKind::LlvmInlineAsm(_)
            | StatementKind::CopyNonOverlapping(_)
            | StatementKind::SetDiscriminant { .. } => {
                return None;
            }
        }
    }

    Some(switch_place)
}
