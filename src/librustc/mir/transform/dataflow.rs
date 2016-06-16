// Copyright 2016 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use mir::repr as mir;
use mir::repr::{BasicBlock, START_BLOCK};
use rustc_data_structures::bitvec::BitVector;
use rustc_data_structures::indexed_vec::Idx;

use mir::transform::lattice::Lattice;

pub trait Transfer<'tcx> {
    type Lattice: Lattice;

    /// The transfer function which given a statement and a fact produces a fact which is true
    /// after the statement.
    fn stmt(&mir::Statement<'tcx>, Self::Lattice) -> Self::Lattice;

    /// The transfer function which given a terminator and a fact produces a fact for each
    /// successor of the terminator.
    ///
    /// Remember, that in backward analysis, terminator only ever has a single successor, therefore
    /// this function may only return a vector with exactly one element in it.
    ///
    /// Corectness precondtition:
    /// * The list of facts produced should only contain the facts for blocks which are successors
    /// of the terminator being transfered.
    fn term(&mir::Terminator<'tcx>, Self::Lattice) -> Vec<Self::Lattice>;
}

pub trait Rewrite<'tcx, T: Transfer<'tcx>> {
    /// The rewrite function which given a statement optionally produces an alternative graph to be
    /// placed in place of the original statement.
    ///
    /// The 2nd BasicBlock *MUST NOT* have the terminator set.
    ///
    /// Correctness precondition:
    /// * transfer_stmt(statement, fact) == transfer_stmt(rewrite_stmt(statement, fact))
    /// that is, given some fact `fact` true before both the statement and relacement graph, and
    /// a fact `fact2` which is true after the statement, the same `fact2` must be true after the
    /// replacement graph too.
    fn stmt(&self, &mir::Statement<'tcx>, &T::Lattice, &mut mir::Mir<'tcx>)
    -> StatementChange<'tcx>;

    /// The rewrite function which given a terminator optionally produces an alternative graph to
    /// be placed in place of the original statement.
    ///
    /// The 2nd BasicBlock *MUST* have the terminator set.
    ///
    /// Correctness precondition:
    /// * transfer_stmt(terminator, fact) == transfer_stmt(rewrite_term(terminator, fact))
    /// that is, given some fact `fact` true before both the terminator and relacement graph, and
    /// a fact `fact2` which is true after the statement, the same `fact2` must be true after the
    /// replacement graph too.
    fn term(&self, &mir::Terminator<'tcx>, &T::Lattice, &mut mir::Mir<'tcx>)
    -> TerminatorChange<'tcx>;

    /// Combine two rewrites using RewriteAndThen combinator.
    fn and_then<R2>(self, other: R2) -> RewriteAndThen<Self, R2> where Self: Sized {
        RewriteAndThen(self, other)
    }
}

/// This combinator has the following behaviour:
///
/// * Rewrite the node with the first rewriter.
///   * if the first rewriter replaced the node, 2nd rewriter is used to rewrite the replacement.
///   * otherwise 2nd rewriter is used to rewrite the original node.
pub struct RewriteAndThen<R1, R2>(R1, R2);

impl<'tcx, T, R1, R2> Rewrite<'tcx, T> for RewriteAndThen<R1, R2>
where T: Transfer<'tcx>, R1: Rewrite<'tcx, T>, R2: Rewrite<'tcx, T> {
    fn stmt(&self, s: &mir::Statement<'tcx>, l: &T::Lattice, c: &mut mir::Mir<'tcx>)
    -> StatementChange<'tcx> {
        let rs = self.0.stmt(s, l, c);
        match rs {
            StatementChange::None => self.1.stmt(s, l, c),
            StatementChange::Remove => StatementChange::Remove,
            StatementChange::Statement(ns) =>
                match self.1.stmt(&ns, l, c) {
                    StatementChange::None => StatementChange::Statement(ns),
                    x => x
                },
            _ => unimplemented!()
        }
    }

    fn term(&self, t: &mir::Terminator<'tcx>, l: &T::Lattice, c: &mut mir::Mir<'tcx>)
    -> TerminatorChange<'tcx> {
        let rt = self.0.term(t, l, c);
        match rt {
            TerminatorChange::None => self.1.term(t, l, c),
            TerminatorChange::Terminator(nt) => match self.1.term(&nt, l, c) {
                TerminatorChange::None => TerminatorChange::Terminator(nt),
                x => x
            },
            _ => unimplemented!()
        }
    }
}

pub enum TerminatorChange<'tcx> {
    /// No change
    None,
    /// Replace with another terminator
    Terminator(mir::Terminator<'tcx>),
    /// Replace with an arbitrary graph
    Graph {
        /// Represents the entry point into the replacement graph
        entry: mir::BasicBlock,
        /// Represents the exit point from the replacement graph. This block must have a set
        /// terminator.
        exit: mir::BasicBlock
    },
}

pub enum StatementChange<'tcx> {
    /// No change
    None,
    /// Remove the statement
    Remove,
    /// Replace with another single statement
    Statement(mir::Statement<'tcx>),
    /// Replace with an arbitrary graph
    Graph {
        /// Represents the entry point into the replacement graph
        entry: mir::BasicBlock,
        /// Represents the exit point from the replacement graph. This block must *not* have a set
        /// terminator.
        exit: mir::BasicBlock
    }
}

/// Facts is a mapping from basic block label (index) to the fact which is true about the first
/// statement in the block.
pub struct Facts<F>(pub Vec<F>);

impl<F: Lattice> Facts<F> {
    pub fn new() -> Facts<F> {
        Facts(vec![])
    }

    fn put(&mut self, index: BasicBlock, value: F) {
        let len = self.0.len();
        self.0.extend((len...index.index()).map(|_| <F as Lattice>::bottom()));
        self.0[index.index()] = value;
    }
}

impl<F: Lattice> ::std::ops::Index<BasicBlock> for Facts<F> {
    type Output = F;
    fn index(&self, index: BasicBlock) -> &F {
        &self.0.get(index.index()).expect("facts indexed immutably and the user is buggy!")
    }
}

impl<F: Lattice> ::std::ops::IndexMut<BasicBlock> for Facts<F> {
    fn index_mut(&mut self, index: BasicBlock) -> &mut F {
        if self.0.get(index.index()).is_none() {
            self.put(index, <F as Lattice>::bottom());
        }
        self.0.get_mut(index.index()).unwrap()
    }
}

/// Analyse and rewrite using dataflow in the forward direction
pub fn analyse_rewrite_forward<'tcx, T, R>(mir: &mut mir::Mir<'tcx>,
                                           fs: Facts<T::Lattice>,
                                           rewrite: &R)
-> Facts<T::Lattice>
where T: Transfer<'tcx>, R: Rewrite<'tcx, T>
{
    let mut queue = BitVector::new(mir.len());
    queue.insert(START_BLOCK.index());

    fixpoint(mir, Direction::Forward, &mut queue, fs, |mir, bb, fact| {
        let mut fact = fact.clone();
        // Swap out the vector of old statements for a duration of statement inspection.
        let mut statements = ::std::mem::replace(&mut mir[bb].statements, Vec::new());
        fact = analyse_rewrite_statements(mir, bb, fact, &mut statements, rewrite);
        // Swap the statements back in.
        mir[bb].statements = statements;

        // Handle the terminator replacement, rewrite and transfer.
        let terminator = mir[bb].terminator.take().expect("invalid terminator state");
        let repl = rewrite.term(&terminator, &fact, mir);
        match repl {
            TerminatorChange::None => {
                mir[bb].terminator = Some(terminator)
            }
            TerminatorChange::Terminator(new_terminator) => {
                mir[bb].terminator = Some(new_terminator);
            }
            TerminatorChange::Graph { entry, .. } => {
                // Replace terminator with statements and terminator of the entry block.
                let stmts = ::std::mem::replace(&mut mir[entry].statements, Vec::new());
                mir[bb].statements.extend(stmts.into_iter());
                mir[bb].terminator = mir[entry].terminator.take();
            }
        }
        // Finally, the facts that are true after terminator are produced by the terminator
        // transfer function
        T::term(mir[bb].terminator(), fact)
    })
}

/// Analyse and rewrite using dataflow in the backward direction starting analysis at the provided
/// blocks.
pub fn analyse_rewrite_backward<'tcx, T, R>(mir: &mut mir::Mir<'tcx>,
                                            fs: Facts<T::Lattice>,
                                            rewrite: &R)
-> Facts<T::Lattice>
where T: Transfer<'tcx>, R: Rewrite<'tcx, T>
{
    let mut queue = BitVector::new(mir.len());
    // very naive way to figure out exit blocks: see whether block has any successors. If not, it
    // is an exit block. This, however does not detect infinite loops...
    for (i, block) in mir.basic_blocks().iter_enumerated() {
        if block.terminator().successors().len() == 0 {
            queue.insert(i.index());
        }
    }
    fixpoint(mir, Direction::Backward, &mut queue, fs, |mir, bb, fact| {
        println!("dataflowing {:?}", bb);
        let mut fact = fact.clone();
        // Remember, this is backward analysis, therefore we must analyse here backwards as well,
        // starting at the terminator and going through the statements backwards. This is
        // essentially a mirror of the code for forward analysis.
        let terminator = mir[bb].terminator.take().expect("invalid terminator state");
        let repl = rewrite.term(&terminator, &fact, mir);
        match repl {
            TerminatorChange::None => {
                mir[bb].terminator = Some(terminator)
            }
            TerminatorChange::Terminator(new_terminator) => {
                mir[bb].terminator = Some(new_terminator);
            }
            TerminatorChange::Graph { entry, .. } => {
                // Replace terminator with statements and terminator of the entry block.
                let stmts = ::std::mem::replace(&mut mir[entry].statements, Vec::new());
                mir[bb].statements.extend(stmts.into_iter());
                mir[bb].terminator = mir[entry].terminator.take();
            }
        }
        fact = {
            let mut term_facts = T::term(mir[bb].terminator(), fact);
            assert!(term_facts.len() == 1, "in backward analysis terminator transfer function \
                                            must return a vector with exactly one element");
            term_facts.pop().unwrap()
        };
        // Swap out the vector of old statements for a duration of statement inspection.
        let mut statements = ::std::mem::replace(&mut mir[bb].statements, Vec::new());
        // In order to keep the code same for both forward and back analysis, we simply reverse the
        // list of statements and reverse it back later.
        statements.reverse();
        fact = analyse_rewrite_statements(mir, bb, fact, &mut statements, rewrite);
        // Swap the statements back in.
        statements.reverse();
        mir[bb].statements = statements;
        vec![fact]
    })
}

fn analyse_rewrite_statements<'tcx, T, R>(mir: &mut mir::Mir<'tcx>,
                                          bb: BasicBlock,
                                          fact: T::Lattice,
                                          statements: &mut Vec<mir::Statement<'tcx>>,
                                          rewrite: &R)
-> T::Lattice
where T: Transfer<'tcx>, R: Rewrite<'tcx, T>
{
    let mut fact = fact;
    let mut statement_index = 0;
    loop {
        if statement_index >= statements.len() { break }
        match rewrite.stmt(&statements[statement_index], &fact, mir) {
            StatementChange::None => {
                fact = T::stmt(&statements[statement_index], fact);
                statement_index += 1;
            }
            StatementChange::Remove => {
                // FIXME: there must be a better way to implement this.
                statements.remove(statement_index);
            }
            StatementChange::Statement(new_stmt) => {
                fact = T::stmt(&new_stmt, fact);
                statements[statement_index] = new_stmt;
                statement_index += 1;
            }
            StatementChange::Graph { entry, exit } => {
                // First split the currently analysed block into two parts. Move the tail to
                // the `exit` block by appending it.
                mir[exit].statements.extend(statements.drain((statement_index + 1)..));
                debug_assert!(mir[exit].terminator.is_none(),
                              "exit block must have no terminator set!");
                mir[exit].terminator = mir[bb].terminator.take();
                // Then pop the current statement, because we are replacing it with the graph.
                statements.pop();
                // Then merge the current block and `entry`, thus producing a valid CFG.
                statements.extend(mir[entry].statements.drain(..));
                mir[bb].terminator = mir[entry].terminator.take();
                // Safeguard for a case where `entry` had no statements in it.
                if statement_index < statements.len() {
                    fact = T::stmt(&statements[statement_index], fact);
                }
            }
        }
    }
    fact
}

enum Direction {
    Forward,
    Backward
}

/// The fixpoint function is the engine of this whole thing.
///
/// The purpose of this function is to stop executing dataflow once the analysis converges to a
/// fixed point.
///
/// The most important argument of these the `f: BF` callback. This callback will get called for
/// a BasicBlock and its associated factset. The function must then produce a list of facts. Number
/// of these facts should match the number and correspond to edges returned by
/// `mir.successors_for(block)` in case of forward analysis and exactly `1` in case of backward
/// analysis.
///
/// Once join operation produces no new facts (i.e. facts do not change anymore), the fixpoint loop
/// terminates, thus completing the analysis.
///
/// Invariant:
/// * None of the already existing blocks in CFG may be modified by `callback`;
fn fixpoint<'tcx, F, BF>(mir: &mut mir::Mir<'tcx>,
                         direction: Direction,
                         queue: &mut BitVector,
                         facts: Facts<F>,
                         callback: BF)
-> Facts<F>
where BF: Fn(&mut mir::Mir<'tcx>, BasicBlock, &F) -> Vec<F>,
      F: Lattice
{
    // FIXME: detect divergence somehow?
    let mut facts = facts;
    let mut mir = mir;

    while let Some(block) = queue.pop() {
        let block = BasicBlock::new(block);
        let new_facts = {
            let fact = &mut facts[block];
            callback(mir, block, fact)
        };

        // Then we record the facts in the correct direction.
        match direction {
            Direction::Forward => {
                let successors = mir[block].terminator().successors();
                // While technically not strictly necessary assertion, it is easy to mess up the
                // zip below with successors and facts being of different lenghts. Either way, I do
                // not a single case where returning less (or even worse, more) facts would be
                // desirable other than for a brittle micro-optimisation.
                assert!(successors.len() == new_facts.len(),
                        "list of facts must match the number of successors");
                for (f, &target) in new_facts.into_iter().zip(successors.iter()) {
                    if Lattice::join(&mut facts[target], &f) {
                        queue.insert(target.index());
                    }
                }
            }
            Direction::Backward => {
                let predecessors = mir.predecessors_for(block);
                assert!(new_facts.len() == 1,
                        "backward fixpoint cannot handle new_facts with length != 1");
                for &target in predecessors.iter() {
                    if Lattice::join(&mut facts[target], &new_facts[0]) {
                        queue.insert(target.index());
                    }
                }
            }
        }
    }
    facts
}
