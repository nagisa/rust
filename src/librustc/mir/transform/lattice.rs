// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::fmt::{Debug, Formatter};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

/// A lattice type for forward and backward dataflow.
///
/// This lattice requires ⊥ to be defined for both forward and backward analyses, however some of
/// the analyses might not need it, therefore it is fine to implement it as a panic (`bug!`).
pub trait Lattice: Clone {
    fn bottom() -> Self;
    fn join(&mut self, other: &Self) -> bool;
}

/// Extend the type with a Top point.
///
/// Lattice extended with a top point follows these rules:
///
/// ```
/// v + v = V::join(v, v)
/// ⊤ + v = ⊤ (no change)
/// v + ⊤ = ⊤
/// ⊤ + ⊤ = ⊤ (no change)
/// ```
///
/// where `v` is the wrapped value and `V` is its type.
#[derive(Clone, PartialEq)]
pub enum WTop<T> {
    Top,
    Value(T)
}

impl<T: Lattice> Lattice for WTop<T> {
    fn bottom() -> Self {
        WTop::Value(<T as Lattice>::bottom())
    }

    fn join(&mut self, other: &Self) -> bool {
        match (self, other) {
            (&mut WTop::Value(ref mut this), &WTop::Value(ref o)) => <T as Lattice>::join(this, o),
            (&mut WTop::Top, _) => false,
            (this, &WTop::Top) => {
                *this = WTop::Top;
                true
            }
        }
    }
}

impl<T: Debug> Debug for WTop<T> {
    fn fmt(&self, f: &mut Formatter) -> ::std::fmt::Result {
        match *self {
            WTop::Top => f.write_str("⊤"),
            WTop::Value(ref t) => <T as Debug>::fmt(t, f)
        }
    }
}

/// Extend the type with a bottom point.
///
/// This guarantees the bottom() of the underlying lattice won’t get called, making this is a
/// useful wrapper for lattices with no obvious bottom value.
///
/// Lattice extended with a bottom point follows these rules:
///
/// ```
/// v + v = V::join(v, v)
/// ⊥ + v = v
/// v + ⊥ = v (no change)
/// ⊥ + ⊥ = ⊥ (no change)
/// ```
///
/// where `v` is the wrapped value and `V` is its type.
#[derive(Clone, PartialEq)]
pub enum WBottom<T> {
    Bottom,
    Value(T)
}

impl<T: Lattice> Lattice for WBottom<T> {
    fn bottom() -> Self {
        WBottom::Bottom
    }

    fn join(&mut self, other: &Self) -> bool {
        match (self, other) {
            (&mut WBottom::Value(ref mut this), &WBottom::Value(ref o)) =>
                <T as Lattice>::join(this, o),
            (_, &WBottom::Bottom) => false,
            (this, o) => {
                *this = o.clone();
                true
            }
        }
    }

}

impl<T: Debug> Debug for WBottom<T> {
    fn fmt(&self, f: &mut Formatter) -> ::std::fmt::Result {
        match *self {
            WBottom::Bottom => f.write_str("⊥"),
            WBottom::Value(ref t) => <T as Debug>::fmt(t, f)
        }
    }
}

/// Extend the type with both bottom and top points.
///
/// Lattice extended with both points follows these rules:
///
/// ```
/// v + v = join(v, v)
/// v + ⊥ = v (no change)
/// v + ⊤ = ⊤
/// ⊥ + v = v
/// ⊥ + ⊥ = ⊥ (no change)
/// ⊥ + ⊤ = ⊤
/// ⊤ + v = ⊤ (no change)
/// ⊤ + ⊤ = ⊤ (no change)
/// ⊤ + ⊥ = ⊤ (no change)
/// ```
///
/// where `v` is the wrapped value and `V` is its type.
type WTopBottom<T> = WTop<WBottom<T>>;


// TODO: should have wrapper, really, letting to pick between union or intersection..
/// A hashmap lattice with union join operation.
impl<K, T, H> Lattice for HashMap<K, T, H>
where K: Clone + Eq + ::std::hash::Hash,
      T: Lattice,
      H: Clone + ::std::hash::BuildHasher + ::std::default::Default
{
    fn bottom() -> Self {
        HashMap::default()
    }
    fn join(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (key, val) in other.iter() {
            match self.entry(key.clone()) {
                Entry::Vacant(e) => {
                    e.insert(val.clone());
                    changed = true
                }
                Entry::Occupied(mut e) => changed |= e.get_mut().join(val)
            }
        }
        changed
    }
}
