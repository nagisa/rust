// Copyright 2012-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use llvm::BasicBlockRef;
use rustc::mir::repr as mir;
use trans::base;
use trans::build;
use trans::common;
use trans::attributes;
use trans::type_of;
use trans::common::Block;
use trans::debuginfo::DebugLoc;
use trans::mir::operand::OperandValue;

use syntax::abi;
use rustc::middle::ty;

use super::MirContext;

impl<'bcx, 'tcx> MirContext<'bcx, 'tcx> {
    pub fn trans_block(&mut self, bb: mir::BasicBlock) {
        debug!("trans_block({:?})", bb);

        let mut bcx = self.bcx(bb);
        let data = self.mir.basic_block_data(bb);

        for statement in &data.statements {
            bcx = self.trans_statement(bcx, statement);
        }

        debug!("trans_block: terminator: {:?}", data.terminator);

        match data.terminator {
            mir::Terminator::Goto { target } => {
                build::Br(bcx, self.llblock(target), DebugLoc::None)
            }

            mir::Terminator::Panic { .. } => {
                unimplemented!()
            }

            mir::Terminator::If { ref cond, targets: (true_bb, false_bb) } => {
                let cond = self.trans_operand(bcx, cond);
                let lltrue = self.llblock(true_bb);
                let llfalse = self.llblock(false_bb);
                build::CondBr(bcx, cond.immediate(), lltrue, llfalse, DebugLoc::None);
            }

            mir::Terminator::Switch { .. } => {
                unimplemented!()
            }

            mir::Terminator::SwitchInt { ref discr, switch_ty, ref values, ref targets } => {
                let (otherwise, targets) = targets.split_last().unwrap();
                let discr = build::Load(bcx, self.trans_lvalue(bcx, discr).llval);
                let switch = build::Switch(bcx, discr, self.llblock(*otherwise), values.len());
                for (value, target) in values.iter().zip(targets) {
                    let llval = self.trans_constval(bcx, value, switch_ty).immediate();
                    let llbb = self.llblock(*target);
                    build::AddCase(switch, llval, llbb)
                }
            }

            mir::Terminator::Diverge => {
                if let Some(llpersonalityslot) = self.llpersonalityslot {
                    let lp = build::Load(bcx, llpersonalityslot);
                    // FIXME(lifetime) base::call_lifetime_end(bcx, self.personality);
                    build::Resume(bcx, lp);
                } else {
                    // This fn never encountered anything fallible, so
                    // a Diverge cannot actually happen. Note that we
                    // do a total hack to ensure that we visit the
                    // DIVERGE block last.
                    build::Unreachable(bcx);
                }
            }

            mir::Terminator::Return => {
                let return_ty = bcx.monomorphize(&self.mir.return_ty);
                base::build_return_block(bcx.fcx, bcx, return_ty, DebugLoc::None);
            }

            mir::Terminator::Call { ref data, targets: (success_target, panic_target) } => {
                let callee = self.trans_operand(bcx, &data.func);
                let attributes = attributes::from_fn_type(bcx.ccx(), callee.ty);
                let ret_dest = self.trans_lvalue(bcx, &data.destination);
                let mut args = Vec::new();

                let (abi, ret_ty) = match callee.ty.sty {
                    ty::TyBareFn(_, ref f) =>
                        (f.abi, bcx.tcx().erase_late_bound_regions(&f.sig.output())),
                    _ => panic!("expected bare rust fn or closure in Call terminator")
                };
                assert!(abi != abi::RustIntrinsic && abi != abi::PlatformIntrinsic);
                let is_rust_fn = abi == abi::Rust || abi == abi::RustCall;

                // The code below decides how to handle the return value.
                if is_rust_fn {
                    let mut copy_retval_into = None;
                    let ret_ty = if let ty::FnConverging(ret_ty) = ret_ty {
                        let llformal_ret_ty = type_of::type_of(bcx.ccx(), ret_ty).ptr_to();
                        let llret_ty = common::val_ty(ret_dest.llval);
                        let llret_slot = if llformal_ret_ty != llret_ty {
                            build::PointerCast(bcx, ret_dest.llval, llformal_ret_ty)
                        } else {
                            ret_dest.llval
                        };
                        if type_of::return_uses_outptr(bcx.ccx(), ret_ty) {
                            args.push(llret_slot);
                            Some(ret_ty)
                        } else {
                            if !common::type_is_zero_size(bcx.ccx(), ret_ty) {
                                copy_retval_into = Some(llret_slot);
                            }
                            Some(ret_ty)
                        }
                    } else { // diverging, no return value
                        None
                    };
                    for arg in &data.args {
                        match self.trans_operand(bcx, arg).val {
                            OperandValue::Ref(v) => args.push(v),
                            OperandValue::Immediate(v) => args.push(v),
                            OperandValue::FatPtr(d, m) => {
                                args.push(d);
                                args.push(m);
                            }
                        }
                    }

                    if panic_target != mir::DIVERGE_BLOCK {
                        build::Invoke(bcx, callee.immediate(), &args[..],
                                      self.llblock(success_target),
                                      self.llblock(panic_target),
                                      Some(attributes), DebugLoc::None);
                        if let Some(_) = copy_retval_into {
                            unimplemented!();
                        }
                    } else {
                        let r = build::Call(bcx, callee.immediate(), &args[..],
                                            Some(attributes), DebugLoc::None);
                        if let Some(rd) = copy_retval_into {
                            let ty = ret_ty.expect("copy_retval_into is Some, but no ret_ty");
                            base::store_ty(bcx, r, rd, ty);
                        }
                        build::Br(bcx, self.llblock(success_target), DebugLoc::None);
                    };
                } else { // non-rust function
                    // bcx = foreign::trans_native_call(bcx,
                    //                                  callee.ty,
                    //                                  llfn,
                    //                                  opt_llretslot.unwrap(),
                    //                                  &llargs[..],
                    //                                  arg_tys,
                    //                                  debug_loc);
                    unimplemented!()
                }
            }
        }
    }

    fn bcx(&self, bb: mir::BasicBlock) -> Block<'bcx, 'tcx> {
        self.blocks[bb.index()]
    }

    fn llblock(&self, bb: mir::BasicBlock) -> BasicBlockRef {
        self.blocks[bb.index()].llbb
    }
}
