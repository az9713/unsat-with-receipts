//! M1: DIMACS parser + recursive DPLL.
//! The parser and CLI conventions survive to M2+. The DPLL solver is
//! disposable scaffolding; M2 replaces it with CDCL.

pub mod cdcl;
pub mod dimacs;
pub mod dpll;

/// A literal in DIMACS convention: nonzero i32, sign = polarity.
pub type Lit = i32;

/// A clause is a disjunction of literals.
pub type Clause = Vec<Lit>;

/// A CNF formula.
#[derive(Debug, Clone, Default)]
pub struct Formula {
    /// Highest variable index seen (>= header value; parser grows it).
    pub num_vars: usize,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Satisfiable, with a model: model[v] is the value of variable v+1.
    Sat(Vec<bool>),
    Unsat,
}
