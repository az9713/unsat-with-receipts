//! M2: CDCL with two-watched literals and 1UIP clause learning.
//!
//! Decision heuristic is naive (first unassigned variable, phase false);
//! M4 adds EVSIDS/VMTF, Luby restarts, and LBD-tiered deletion.
//!
//! Proof choke point: every clause the solver adds or deletes after the
//! input formula goes through `proof_add` / `proof_delete`. M3 makes these
//! write DRAT lines; nothing else may mutate the clause database.

use crate::{Formula, Verdict};

/// Internal literal: var*2 + (1 if negative). Vars are 0-based.
type L = u32;
type Var = u32;
type ClauseRef = usize;

fn lit(var: Var, neg: bool) -> L {
    var * 2 + neg as u32
}
fn var(l: L) -> Var {
    l / 2
}
fn neg(l: L) -> L {
    l ^ 1
}
fn from_dimacs(d: i32) -> L {
    lit(d.unsigned_abs() - 1, d < 0)
}
fn to_dimacs(l: L) -> i32 {
    let v = var(l) as i32 + 1;
    if l & 1 == 1 {
        -v
    } else {
        v
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Val {
    True,
    False,
    Undef,
}

fn val(assign: &[Val], l: L) -> Val {
    match assign[var(l) as usize] {
        Val::Undef => Val::Undef,
        Val::True => {
            if l & 1 == 0 {
                Val::True
            } else {
                Val::False
            }
        }
        Val::False => {
            if l & 1 == 0 {
                Val::False
            } else {
                Val::True
            }
        }
    }
}

struct Clause {
    lits: Vec<L>,
}

pub struct Solver {
    clauses: Vec<Clause>,
    /// watches[l] = clause refs currently watching literal l.
    watches: Vec<Vec<ClauseRef>>,
    assign: Vec<Val>,
    level: Vec<u32>,
    reason: Vec<Option<ClauseRef>>,
    trail: Vec<L>,
    trail_lim: Vec<usize>,
    qhead: usize,
    num_vars: usize,
    /// Set when the input or level-0 propagation is already contradictory.
    root_conflict: bool,
    /// DRAT proof lines (clause add/delete), filled from M3 on.
    pub proof: Vec<String>,
    proof_enabled: bool,
}

impl Solver {
    pub fn new(f: &Formula, proof_enabled: bool) -> Solver {
        let n = f.num_vars;
        let mut s = Solver {
            clauses: Vec::new(),
            watches: vec![Vec::new(); 2 * n],
            assign: vec![Val::Undef; n],
            level: vec![0; n],
            reason: vec![None; n],
            trail: Vec::new(),
            trail_lim: Vec::new(),
            qhead: 0,
            num_vars: n,
            root_conflict: false,
            proof: Vec::new(),
            proof_enabled,
        };
        for c in &f.clauses {
            s.add_input_clause(c);
            if s.root_conflict {
                break;
            }
        }
        s
    }

    fn value(&self, l: L) -> Val {
        val(&self.assign, l)
    }

    fn enqueue(&mut self, l: L, reason: Option<ClauseRef>) {
        let v = var(l) as usize;
        self.assign[v] = if l & 1 == 0 { Val::True } else { Val::False };
        self.level[v] = self.trail_lim.len() as u32;
        self.reason[v] = reason;
        self.trail.push(l);
    }

    /// Input clauses: dedupe, drop tautologies, no proof line (they are
    /// part of the formula, not derived).
    fn add_input_clause(&mut self, dimacs: &[i32]) {
        let mut lits: Vec<L> = dimacs.iter().map(|&d| from_dimacs(d)).collect();
        lits.sort_unstable();
        lits.dedup();
        if lits.windows(2).any(|w| w[0] == neg(w[1])) {
            return; // tautology
        }
        // Level-0 simplification of the input only (safe without proof lines).
        lits.retain(|&l| self.value(l) != Val::False);
        if lits.iter().any(|&l| self.value(l) == Val::True) {
            return;
        }
        match lits.len() {
            0 => self.root_conflict = true,
            1 => {
                self.enqueue(lits[0], None);
                if self.propagate().is_some() {
                    self.root_conflict = true;
                }
            }
            _ => {
                self.attach(Clause { lits });
            }
        }
    }

    /// The proof choke point for derived clauses. M3 writes a DRAT "add".
    fn proof_add(&mut self, lits: &[L]) {
        if self.proof_enabled {
            let mut line: String = lits
                .iter()
                .map(|&l| to_dimacs(l).to_string())
                .collect::<Vec<_>>()
                .join(" ");
            if !line.is_empty() {
                line.push(' ');
            }
            line.push('0');
            self.proof.push(line);
        }
    }

    fn attach(&mut self, c: Clause) -> ClauseRef {
        debug_assert!(c.lits.len() >= 2);
        let cr = self.clauses.len();
        self.watches[c.lits[0] as usize].push(cr);
        self.watches[c.lits[1] as usize].push(cr);
        self.clauses.push(c);
        cr
    }

    /// Two-watched-literal propagation. Returns a conflicting clause ref.
    fn propagate(&mut self) -> Option<ClauseRef> {
        while self.qhead < self.trail.len() {
            let p = self.trail[self.qhead];
            self.qhead += 1;
            let false_lit = neg(p);
            let mut ws = std::mem::take(&mut self.watches[false_lit as usize]);
            let mut i = 0;
            while i < ws.len() {
                let cr = ws[i];
                let c = &mut self.clauses[cr];
                // Ensure the false literal is at position 1.
                if c.lits[0] == false_lit {
                    c.lits.swap(0, 1);
                }
                let first = c.lits[0];
                if val(&self.assign, first) == Val::True {
                    i += 1;
                    continue;
                }
                // Look for a new literal to watch.
                let mut moved = false;
                for k in 2..c.lits.len() {
                    if val(&self.assign, c.lits[k]) != Val::False {
                        c.lits.swap(1, k);
                        let new_watch = c.lits[1];
                        self.watches[new_watch as usize].push(cr);
                        ws.swap_remove(i);
                        moved = true;
                        break;
                    }
                }
                if moved {
                    continue;
                }
                if self.value(first) == Val::False {
                    // Conflict: restore remaining watches.
                    self.watches[false_lit as usize] = ws;
                    return Some(cr);
                }
                // Unit: first is the asserted literal.
                self.enqueue(first, Some(cr));
                i += 1;
            }
            self.watches[false_lit as usize] = ws;
        }
        None
    }

    /// 1UIP conflict analysis. Returns (learned clause, backjump level).
    /// learned[0] is the asserting literal.
    fn analyze(&mut self, confl: ClauseRef) -> (Vec<L>, u32) {
        let cur_level = self.trail_lim.len() as u32;
        let mut seen = vec![false; self.num_vars];
        let mut learned: Vec<L> = Vec::new();
        let mut counter = 0usize;
        let mut p: Option<L> = None;
        let mut idx = self.trail.len();
        let mut cr = confl;

        loop {
            for &q in &self.clauses[cr].lits {
                // Skip the literal being resolved on (the reason's asserted lit).
                if Some(q) == p {
                    continue;
                }
                let v = var(q) as usize;
                if !seen[v] && self.level[v] > 0 {
                    seen[v] = true;
                    if self.level[v] == cur_level {
                        counter += 1;
                    } else {
                        learned.push(q);
                    }
                }
            }
            // Walk the trail backwards to the next marked literal.
            loop {
                idx -= 1;
                if seen[var(self.trail[idx]) as usize] {
                    break;
                }
            }
            let lit_p = self.trail[idx];
            seen[var(lit_p) as usize] = false;
            counter -= 1;
            if counter == 0 {
                p = Some(lit_p);
                break;
            }
            p = Some(lit_p);
            cr = self.reason[var(lit_p) as usize].expect("non-decision has a reason");
        }
        learned.insert(0, neg(p.unwrap()));

        // Backjump level: highest level among learned[1..].
        let bt = learned[1..]
            .iter()
            .map(|&l| self.level[var(l) as usize])
            .max()
            .unwrap_or(0);
        // Put a literal of the backjump level at position 1 (watch invariant).
        if learned.len() > 1 {
            let pos = 1 + learned[1..]
                .iter()
                .position(|&l| self.level[var(l) as usize] == bt)
                .unwrap();
            learned.swap(1, pos);
        }
        (learned, bt)
    }

    fn cancel_until(&mut self, level: u32) {
        while self.trail_lim.len() as u32 > level {
            let lim = self.trail_lim.pop().unwrap();
            while self.trail.len() > lim {
                let l = self.trail.pop().unwrap();
                let v = var(l) as usize;
                self.assign[v] = Val::Undef;
                self.reason[v] = None;
            }
        }
        self.qhead = self.trail.len();
    }

    pub fn solve(&mut self) -> Verdict {
        if self.root_conflict {
            self.proof_add(&[]);
            return Verdict::Unsat;
        }
        if self.propagate().is_some() {
            self.proof_add(&[]);
            return Verdict::Unsat;
        }
        loop {
            match self.propagate() {
                Some(confl) => {
                    if self.trail_lim.is_empty() {
                        self.proof_add(&[]);
                        return Verdict::Unsat;
                    }
                    let (learned, bt) = self.analyze(confl);
                    self.proof_add(&learned);
                    self.cancel_until(bt);
                    if learned.len() == 1 {
                        self.enqueue(learned[0], None);
                    } else {
                        let cr = self.attach(Clause { lits: learned });
                        let assert_lit = self.clauses[cr].lits[0];
                        self.enqueue(assert_lit, Some(cr));
                    }
                }
                None => {
                    // Decide.
                    match self.assign.iter().position(|&a| a == Val::Undef) {
                        None => {
                            let model = self
                                .assign
                                .iter()
                                .map(|&a| a == Val::True)
                                .collect();
                            return Verdict::Sat(model);
                        }
                        Some(v) => {
                            self.trail_lim.push(self.trail.len());
                            // ponytail: naive first-var/phase-false decision;
                            // EVSIDS/VMTF land in M4.
                            self.enqueue(lit(v as Var, true), None);
                        }
                    }
                }
            }
        }
    }
}

pub fn solve(f: &Formula) -> Verdict {
    Solver::new(f, false).solve()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimacs::parse;
    use crate::dpll;

    fn check_model(f: &Formula, model: &[bool]) {
        for clause in &f.clauses {
            assert!(
                clause
                    .iter()
                    .any(|&l| model[l.unsigned_abs() as usize - 1] == (l > 0)),
                "model does not satisfy {clause:?}"
            );
        }
    }

    fn run(input: &str) -> (Formula, Verdict) {
        let f = parse(input).unwrap();
        let v = solve(&f);
        (f, v)
    }

    #[test]
    fn hand_cases_match_dpll_suite() {
        for (input, expect_sat) in [
            ("p cnf 0 0\n", true),
            ("0\n", false),
            ("1 0\n-1 2 0\n-2 3 0\n", true),
            ("1 2 0\n-1 3 0\n-3 -2 1 0\n", true),
            ("1 0\n-1 0\n", false),
            ("1 0\n2 0\n-1 -2 0\n", false),
            (
                "1 2 0\n3 4 0\n5 6 0\n-1 -3 0\n-1 -5 0\n-3 -5 0\n-2 -4 0\n-2 -6 0\n-4 -6 0\n",
                false,
            ),
        ] {
            let (f, v) = run(input);
            match (&v, expect_sat) {
                (Verdict::Sat(m), true) => check_model(&f, m),
                (Verdict::Unsat, false) => {}
                _ => panic!("wrong verdict on {input:?}: {v:?}"),
            }
        }
    }

    #[test]
    fn agrees_with_dpll_on_random_3sat() {
        // In-crate xorshift fuzz: 500 small instances vs the M1 DPLL oracle.
        let mut state = 0x243F6A8885A308D3u64;
        let mut rng = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..500 {
            let n = 3 + (rng() % 10) as usize; // 3..12 vars
            let m = (n as f64 * 4.26).round() as usize;
            let mut f = Formula {
                num_vars: n,
                clauses: Vec::new(),
            };
            for _ in 0..m {
                let mut c = Vec::new();
                for _ in 0..3 {
                    let v = (rng() % n as u64) as i32 + 1;
                    let s = if rng() & 1 == 0 { 1 } else { -1 };
                    c.push(v * s);
                }
                f.clauses.push(c);
            }
            let a = solve(&f);
            let b = dpll::solve(&f);
            match (&a, &b) {
                (Verdict::Sat(m1), Verdict::Sat(_)) => check_model(&f, m1),
                (Verdict::Unsat, Verdict::Unsat) => {}
                _ => panic!("divergence at round {round}: cdcl={a:?} dpll={b:?} f={f:?}"),
            }
        }
    }
}
