#![feature(control_flow_enum)]

use std::ops::ControlFlow;

// EMIT_MIR separate_const_switch.too_complex.SeparateConstSwitch.diff
fn too_complex<T, E>(x: Result<T, E>) -> Option<T> {
    // The pass should break the outer match into
    // two blocks that only have one parent each.
    // Parents are one of the two branches of the first
    // match, so a later pass can propagate constants.
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
