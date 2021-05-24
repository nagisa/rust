//! A pass that copies switch-terminated blocks into
//! another copy for each parent that set the value
//! being switched over to a constant.

use crate::transform::MirPass;
use rustc_middle::mir::*;
use rustc_middle::ty::TyCtxt;
use smallvec::SmallVec;

pub struct SeparateConstSwitch;

impl<'tcx> MirPass<'tcx> for SeparateConstSwitch {
    fn run_pass(&self, tcx: TyCtxt<'tcx>, body: &mut Body<'tcx>) {
        if tcx.sess.mir_opt_level() < 4 {
            return;
        }

        separate_const_switch(body);
    }
}

pub fn separate_const_switch<'tcx>(body: &mut Body<'tcx>) {
    let mut new_edges: SmallVec<[(BasicBlock, BasicBlock); 6]> = SmallVec::new();
    let predecessors = body.predecessors();
    'block_iter: for (block_id, block) in body.basic_blocks().iter_enumerated() {
        if let TerminatorKind::SwitchInt {
            discr: Operand::Copy(switch_place) | Operand::Move(switch_place),
            ..
        } = block.terminator().kind
        {
            // if the block is on an unwind path, do not
            // apply the optimization as unwind paths
            // rely on a unique parent invariant
            if block.is_cleanup {
                continue 'block_iter;
            }

            // if the block has fewer than 2 predecessors, ignore it
            // we could maybe chain blocks that have exactly one
            // predecessor, but for now we ignore that
            if predecessors[block_id].len() < 2 {
                continue 'block_iter;
            }

            // first, let's find a non-const place
            // that determines the result of the switch
            if let Some(switch_place) = find_determining_place(switch_place, block) {
                // we now have an input place for which it would
                // be interesting if predecessors assigned it from a const

                let mut predecessors_left = predecessors[block_id].len();
                'predec_iter: for predecessor_id in predecessors[block_id].iter().copied() {
                    if let Some(predecessor) = body.basic_blocks().get(predecessor_id) {
                        // first we make sure the predecessor jumps
                        // in a reasonable way
                        match &predecessor.terminator().kind {
                            // the following terminators are
                            // unconditionally valid
                            TerminatorKind::Goto { .. } | TerminatorKind::SwitchInt { .. } => {}

                            TerminatorKind::FalseEdge { real_target, .. } => {
                                if *real_target != block_id {
                                    continue 'predec_iter;
                                }
                            }

                            // the following terminators are not allowed
                            TerminatorKind::Resume
                            | TerminatorKind::Drop { .. }
                            | TerminatorKind::DropAndReplace { .. }
                            | TerminatorKind::Call { .. }
                            | TerminatorKind::Assert { .. }
                            | TerminatorKind::FalseUnwind { .. }
                            | TerminatorKind::Yield { .. }
                            | TerminatorKind::Abort
                            | TerminatorKind::Return
                            | TerminatorKind::Unreachable
                            | TerminatorKind::InlineAsm { .. }
                            | TerminatorKind::GeneratorDrop => {
                                continue 'predec_iter;
                            }
                        }

                        if is_likely_const(switch_place, predecessor) {
                            new_edges.push((predecessor_id, block_id));
                            predecessors_left -= 1;
                            if predecessors_left < 2 {
                                // there is no point in duplicating anymore
                                break 'predec_iter;
                            }
                        }
                    }
                }
            }
        }
    }

    let blocks = body.basic_blocks_mut();
    for (pred_id, target_id) in new_edges {
        if let Some(new_block) = blocks.get(target_id).cloned() {
            let new_block_id = blocks.push(new_block);
            if let Some(terminator) = blocks.get_mut(pred_id).map(|x| x.terminator_mut()) {
                match terminator.kind {
                    TerminatorKind::Goto { ref mut target } => {
                        *target = new_block_id;
                    }

                    TerminatorKind::FalseEdge { ref mut real_target, .. } => {
                        if *real_target == target_id {
                            *real_target = new_block_id;
                        }
                    }

                    TerminatorKind::SwitchInt { ref mut targets, .. } => {
                        targets.all_targets_mut().iter_mut().for_each(|x| {
                            if *x == target_id {
                                *x = new_block_id;
                            }
                        });
                    }

                    TerminatorKind::Resume
                    | TerminatorKind::Abort
                    | TerminatorKind::Return
                    | TerminatorKind::Unreachable
                    | TerminatorKind::GeneratorDrop
                    | TerminatorKind::Assert { .. }
                    | TerminatorKind::DropAndReplace { .. }
                    | TerminatorKind::FalseUnwind { .. }
                    | TerminatorKind::Drop { .. }
                    | TerminatorKind::Call { .. }
                    | TerminatorKind::InlineAsm { .. }
                    | TerminatorKind::Yield { .. } => {
                        let kind = terminator.kind.clone();
                        span_bug!(
                            body.span,
                            "basic block terminator had unexpected kind {:?}",
                            kind
                        )
                    }
                }
            }
        }
    }
}

/// This function describes a rough heuristic guessing
/// whether a place is last set with a const within the block.
/// Notably, it will be overly pessimistic in cases that are already
/// not handled by `separate_const_switch`.
fn is_likely_const<'tcx>(mut tracked_place: Place<'tcx>, block: &BasicBlockData<'tcx>) -> bool {
    for statement in block.statements.iter().rev() {
        match &statement.kind {
            StatementKind::Assign(assign) => {
                if assign.0 == tracked_place {
                    match assign.1 {
                        // these rvalues are definitely constant
                        Rvalue::Use(Operand::Constant(_))
                        | Rvalue::Ref(_, _, _)
                        | Rvalue::AddressOf(_, _)
                        | Rvalue::Cast(_, Operand::Constant(_), _)
                        | Rvalue::NullaryOp(_, _)
                        | Rvalue::UnaryOp(_, Operand::Constant(_)) => return true,

                        // these rvalues make things ambiguous
                        Rvalue::Repeat(_, _)
                        | Rvalue::ThreadLocalRef(_)
                        | Rvalue::Len(_)
                        | Rvalue::BinaryOp(_, _)
                        | Rvalue::CheckedBinaryOp(_, _)
                        | Rvalue::Aggregate(_, _) => return false,

                        // these rvalues move the place to track
                        Rvalue::Cast(_, Operand::Copy(place) | Operand::Move(place), _)
                        | Rvalue::Use(Operand::Copy(place) | Operand::Move(place))
                        | Rvalue::UnaryOp(_, Operand::Copy(place) | Operand::Move(place))
                        | Rvalue::Discriminant(place) => tracked_place = place,
                    }
                }
            }

            StatementKind::SetDiscriminant { place, .. } => {
                if **place == tracked_place {
                    return true;
                }
            }

            StatementKind::LlvmInlineAsm(_) | StatementKind::CopyNonOverlapping(_) => return false,

            StatementKind::FakeRead(_)
            | StatementKind::StorageLive(_)
            | StatementKind::Retag(_, _)
            | StatementKind::AscribeUserType(_, _)
            | StatementKind::Coverage(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Nop => {}
        }
    }

    // If no good reason for the place to be const is found,
    // give up. We could maybe go up predecessors, but in
    // most cases giving up now should be sufficient.
    false
}

/// Finds a unique place that entirely determines the value
/// of `switch_place`, if it exists. This is only a heuristic.
/// Ideally we would like to track multiple determining places
/// for some edge cases, but one is enough for a lot of situations.
fn find_determining_place<'tcx>(
    mut switch_place: Place<'tcx>,
    block: &BasicBlockData<'tcx>,
) -> Option<Place<'tcx>> {
    let mut tracking_only_discriminant = false;
    for statement in block.statements.iter().rev() {
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
                    | Rvalue::Aggregate(_, _)

                    // The following rvalues definitely mean we cannot
                    // or should not apply this optimization
                    | Rvalue::Use(Operand::Constant(_))
                    | Rvalue::Repeat(_, _)
                    | Rvalue::ThreadLocalRef(_)
                    | Rvalue::AddressOf(_, _)
                    | Rvalue::NullaryOp(_, _)
                    | Rvalue::UnaryOp(_, Operand::Constant(_))
                    | Rvalue::Cast(_, Operand::Constant(_), _)
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
