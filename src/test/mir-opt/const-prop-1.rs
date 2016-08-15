// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
fn main() {
    let x = 10;
    let y = 20;
    if x == 12 {
        [42u8][y];
    } else {
        return;
    }
}

// END RUST SOURCE
// START rustc.node4.ConstPropagate.before.mir
// bb0: {
//     var0 = const 10i32;
//     var1 = const 20usize;
//     tmp1 = var0;
//     tmp0 = Eq(tmp1, const 12i32);
//     if(tmp0) -> [true: bb1, false: bb2];
// }
//
// bb1: {
//     tmp4 = [const 42u8];
//     tmp5 = var1;
//     tmp6 = Len(tmp4);
//     tmp7 = Lt(tmp5, tmp6);
//     assert(tmp7, "index out of bounds: the len is {} but the index is {}", tmp6, tmp5) -> bb3;
// }
//
// bb2: {
//     return = ();
//     goto -> bb4;
// }
//
// bb3: {
//     tmp3 = tmp4[tmp5];
//     tmp2 = tmp3;
//     return = ();
//     goto -> bb4;
// }
// END rustc.node4.ConstPropagate.before.mir
// START rustc.node4.DeadCode.after.mir
// bb0: {
//     goto -> bb1;
// }
//
// bb1: {
//     return = ();
//     return;
// }
// END rustc.node4.DeadCode.after.mir
