#![feature(control_flow_enum)]

use std::ops::ControlFlow;

// EMIT_MIR separate_const_switch.too_complex.SeparateConstSwitch.diff
// EMIT_MIR separate_const_switch.too_complex.ConstProp.diff
fn too_complex<T, E>(x: Result<T, E>) -> Option<T> {
    // we want this construction to be reduced to
    // a single, direct match. to do so, we want
    // to have a pass that will copy the second
    // switch to a separate block. this new block
    // will only be targetted by blocks that fill
    // the condition with a const, so a later
    // pass can optimize it away.
    match { 
        match x {
            Ok(v) => ControlFlow::Continue(v),
            Err(r) => ControlFlow::Break(r),
        }
    } {
        ControlFlow::Continue(v) => Some(v),
        ControlFlow::Break(r) => None,
    }
}

fn main() {
    too_complex::<i32, u32>(Ok(0));
}
