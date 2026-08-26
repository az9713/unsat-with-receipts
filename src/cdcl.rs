//! CDCL with two-watched literals, 1UIP learning, Luby restarts,
//! EVSIDS or VMTF decision heuristics (flag), phase saving, and
//! LBD-tiered learned-clause deletion (glue <= 2 kept forever).
//!
//! Proof choke point: every clause the solver derives or deletes goes
//! through `proof_add` / `proof_delete`. Nothing else may mutate the
//! clause database. Input clauses are not part of the proof.

use crate::{Formula, Verdict};
use std::time::Instant;

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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Heur {
    Evsids,
    Vmtf,
}

struct Clause {
    lits: Vec<L>,
    learned: bool,
    deleted: bool,
    lbd: u32,
}

const NONE: u32 = u32::MAX;

pub struct Solver {
    clauses: Vec<Clause>,
    watches: Vec<Vec<ClauseRef>>,
    assign: Vec<Val>,
    level: Vec<u32>,
    reason: Vec<Option<ClauseRef>>,
    trail: Vec<L>,
    trail_lim: Vec<usize>,
    qhead: usize,
    num_vars: usize,
    root_conflict: bool,
    pub proof: Vec<String>,
    proof_enabled: bool,

    heur: Heur,
    saved_phase: Vec<bool>,
    // EVSIDS: indexed max-heap over activity.
    activity: Vec<f64>,
    var_inc: f64,
    heap: Vec<Var>,
    heap_idx: Vec<u32>, // NONE = not in heap
    // VMTF: doubly-linked recency list + search cursor.
    vm_next: Vec<u32>,
    vm_prev: Vec<u32>,
    vm_head: u32,
    vm_cursor: u32,

    conflicts: u64,
    restart_num: u64,
    conflicts_since_restart: u64,
    learned_live: usize,
    reduce_limit: usize,
    deadline: Option<Instant>,
    pub stats_restarts: u64,
    pub stats_deleted: u64,
}

fn luby(mut i: u64) -> u64 {
    // Luby sequence 1,1,2,1,1,2,4,... (1-based index).
    loop {
        let mut k = 1u64;
        while (1u64 << k) - 1 < i {
            k += 1;
        }
        if (1u64 << k) - 1 == i {
            return 1u64 << (k - 1);
        }
        i -= (1u64 << (k - 1)) - 1;
    }
}

impl Solver {
    pub fn new(f: &Formula, proof_enabled: bool) -> Solver {
        Solver::with_config(f, proof_enabled, Heur::Evsids, None)
    }

    pub fn with_config(
        f: &Formula,
        proof_enabled: bool,
        heur: Heur,
        deadline: Option<Instant>,
    ) -> Solver {
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
            heur,
            saved_phase: vec![false; n],
            activity: vec![0.0; n],
            var_inc: 1.0,
            heap: Vec::with_capacity(n),
            heap_idx: vec![NONE; n],
            vm_next: vec![NONE; n],
            vm_prev: vec![NONE; n],
            vm_head: NONE,
            vm_cursor: NONE,
            conflicts: 0,
            restart_num: 1,
            conflicts_since_restart: 0,
            learned_live: 0,
            reduce_limit: 2000,
            deadline,
            stats_restarts: 0,
            stats_deleted: 0,
        };
        for v in 0..n as Var {
            s.heap_insert(v);
            // VMTF initial order: variable index, head = 0.
            if v + 1 < n as Var {
                s.vm_next[v as usize] = v + 1;
            }
            if v > 0 {
                s.vm_prev[v as usize] = v - 1;
            }
        }
        if n > 0 {
            s.vm_head = 0;
            s.vm_cursor = 0;
        }
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

    // ----- EVSIDS heap -----

    fn heap_less(&self, a: Var, b: Var) -> bool {
        self.activity[a as usize] > self.activity[b as usize]
    }

    fn heap_sift_up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            if self.heap_less(self.heap[i], self.heap[p]) {
                self.heap.swap(i, p);
                self.heap_idx[self.heap[i] as usize] = i as u32;
                self.heap_idx[self.heap[p] as usize] = p as u32;
                i = p;
            } else {
                break;
            }
        }
    }

    fn heap_sift_down(&mut self, mut i: usize) {
        loop {
            let l = 2 * i + 1;
            let r = 2 * i + 2;
            let mut best = i;
            if l < self.heap.len() && self.heap_less(self.heap[l], self.heap[best]) {
                best = l;
            }
            if r < self.heap.len() && self.heap_less(self.heap[r], self.heap[best]) {
                best = r;
            }
            if best == i {
                break;
            }
            self.heap.swap(i, best);
            self.heap_idx[self.heap[i] as usize] = i as u32;
            self.heap_idx[self.heap[best] as usize] = best as u32;
            i = best;
        }
    }

    fn heap_insert(&mut self, v: Var) {
        if self.heap_idx[v as usize] != NONE {
            return;
        }
        self.heap.push(v);
        let i = self.heap.len() - 1;
        self.heap_idx[v as usize] = i as u32;
        self.heap_sift_up(i);
    }

    fn heap_pop(&mut self) -> Option<Var> {
        if self.heap.is_empty() {
            return None;
        }
        let top = self.heap[0];
        self.heap_idx[top as usize] = NONE;
        let last = self.heap.pop().unwrap();
        if !self.heap.is_empty() {
            self.heap[0] = last;
            self.heap_idx[last as usize] = 0;
            self.heap_sift_down(0);
        }
        Some(top)
    }

    fn bump_var(&mut self, v: Var) {
        match self.heur {
            Heur::Evsids => {
                self.activity[v as usize] += self.var_inc;
                if self.activity[v as usize] > 1e100 {
                    for a in &mut self.activity {
                        *a *= 1e-100;
                    }
                    self.var_inc *= 1e-100;
                }
                let idx = self.heap_idx[v as usize];
                if idx != NONE {
                    self.heap_sift_up(idx as usize);
                }
            }
            Heur::Vmtf => {
                // Move to front.
                if self.vm_head == v {
                    return;
                }
                let (p, n) = (self.vm_prev[v as usize], self.vm_next[v as usize]);
                if p != NONE {
                    self.vm_next[p as usize] = n;
                }
                if n != NONE {
                    self.vm_prev[n as usize] = p;
                }
                if self.vm_cursor == v {
                    self.vm_cursor = if n != NONE { n } else { p };
                }
                self.vm_prev[v as usize] = NONE;
                self.vm_next[v as usize] = self.vm_head;
                if self.vm_head != NONE {
                    self.vm_prev[self.vm_head as usize] = v;
                }
                self.vm_head = v;
                self.vm_cursor = v;
            }
        }
    }

    fn decay(&mut self) {
        if self.heur == Heur::Evsids {
            self.var_inc /= 0.95;
        }
    }

    fn pick_branch_var(&mut self) -> Option<Var> {
        match self.heur {
            Heur::Evsids => {
                while let Some(v) = self.heap_pop() {
                    if self.assign[v as usize] == Val::Undef {
                        return Some(v);
                    }
                }
                None
            }
            Heur::Vmtf => {
                let mut c = self.vm_cursor;
                while c != NONE {
                    if self.assign[c as usize] == Val::Undef {
                        self.vm_cursor = c;
                        return Some(c);
                    }
                    c = self.vm_next[c as usize];
                }
                None
            }
        }
    }

    // ----- assignment / trail -----

    fn enqueue(&mut self, l: L, reason: Option<ClauseRef>) {
        let v = var(l) as usize;
        self.assign[v] = if l & 1 == 0 { Val::True } else { Val::False };
        self.level[v] = self.trail_lim.len() as u32;
        self.reason[v] = reason;
        self.trail.push(l);
    }

    fn add_input_clause(&mut self, dimacs: &[i32]) {
        let mut lits: Vec<L> = dimacs.iter().map(|&d| from_dimacs(d)).collect();
        lits.sort_unstable();
        lits.dedup();
        if lits.windows(2).any(|w| w[0] == neg(w[1])) {
            return; // tautology
        }
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
                self.attach(Clause {
                    lits,
                    learned: false,
                    deleted: false,
                    lbd: 0,
                });
            }
        }
    }

    // ----- proof choke point -----

    fn proof_line(&mut self, prefix: &str, lits: &[L]) {
        if !self.proof_enabled {
            return;
        }
        let mut line = String::from(prefix);
        for &l in lits {
            line.push_str(&to_dimacs(l).to_string());
            line.push(' ');
        }
        line.push('0');
        self.proof.push(line);
    }

    fn proof_add(&mut self, lits: &[L]) {
        self.proof_line("", lits);
    }

    fn proof_delete(&mut self, lits: &[L]) {
        self.proof_line("d ", lits);
    }

    // ----- clause DB -----

    fn attach(&mut self, c: Clause) -> ClauseRef {
        debug_assert!(c.lits.len() >= 2);
        let cr = self.clauses.len();
        self.watches[c.lits[0] as usize].push(cr);
        self.watches[c.lits[1] as usize].push(cr);
        self.clauses.push(c);
        cr
    }

    /// Delete learned clauses with LBD > 2, worst half by LBD (older first
    /// on ties). Clauses that are current reasons are kept.
    /// ponytail: tombstone deletion, watch refs dropped lazily in propagate;
    /// arena memory is never compacted — fine at this scale.
    fn reduce_db(&mut self) {
        let mut cands: Vec<(u32, ClauseRef)> = Vec::new();
        for (cr, c) in self.clauses.iter().enumerate() {
            if c.learned && !c.deleted && c.lbd > 2 {
                let asserted = c.lits[0];
                let is_reason = self.assign[var(asserted) as usize] != Val::Undef
                    && self.reason[var(asserted) as usize] == Some(cr);
                if !is_reason {
                    cands.push((c.lbd, cr));
                }
            }
        }
        cands.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let n_del = cands.len() / 2;
        for &(_, cr) in cands.iter().take(n_del) {
            let lits = self.clauses[cr].lits.clone();
            self.clauses[cr].deleted = true;
            self.learned_live -= 1;
            self.stats_deleted += 1;
            self.proof_delete(&lits);
        }
        self.reduce_limit += 300;
    }

    // ----- propagation -----

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
                if c.deleted {
                    ws.swap_remove(i);
                    continue;
                }
                if c.lits[0] == false_lit {
                    c.lits.swap(0, 1);
                }
                let first = c.lits[0];
                if val(&self.assign, first) == Val::True {
                    i += 1;
                    continue;
                }
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
                if val(&self.assign, first) == Val::False {
                    self.watches[false_lit as usize] = ws;
                    return Some(cr);
                }
                self.enqueue(first, Some(cr));
                i += 1;
            }
            self.watches[false_lit as usize] = ws;
        }
        None
    }

    // ----- conflict analysis -----

    fn analyze(&mut self, confl: ClauseRef) -> (Vec<L>, u32, u32) {
        let cur_level = self.trail_lim.len() as u32;
        let mut seen = vec![false; self.num_vars];
        let mut learned: Vec<L> = Vec::new();
        let mut counter = 0usize;
        let mut p: Option<L> = None;
        let mut idx = self.trail.len();
        let mut cr = confl;

        loop {
            let cl_lits = self.clauses[cr].lits.clone();
            for &q in &cl_lits {
                // Skip the literal being resolved on (the reason's asserted lit).
                if Some(q) == p {
                    continue;
                }
                let v = var(q) as usize;
                if !seen[v] && self.level[v] > 0 {
                    seen[v] = true;
                    self.bump_var(v as Var);
                    if self.level[v] == cur_level {
                        counter += 1;
                    } else {
                        learned.push(q);
                    }
                }
            }
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

        let bt = learned[1..]
            .iter()
            .map(|&l| self.level[var(l) as usize])
            .max()
            .unwrap_or(0);
        if learned.len() > 1 {
            let pos = 1 + learned[1..]
                .iter()
                .position(|&l| self.level[var(l) as usize] == bt)
                .unwrap();
            learned.swap(1, pos);
        }

        // LBD: number of distinct decision levels in the learned clause.
        let mut levels: Vec<u32> = learned
            .iter()
            .map(|&l| self.level[var(l) as usize])
            .collect();
        levels.sort_unstable();
        levels.dedup();
        let lbd = levels.len() as u32;

        (learned, bt, lbd)
    }

    fn cancel_until(&mut self, level: u32) {
        while self.trail_lim.len() as u32 > level {
            let lim = self.trail_lim.pop().unwrap();
            while self.trail.len() > lim {
                let l = self.trail.pop().unwrap();
                let v = var(l) as usize;
                self.saved_phase[v] = self.assign[v] == Val::True;
                self.assign[v] = Val::Undef;
                self.reason[v] = None;
                if self.heur == Heur::Evsids {
                    self.heap_insert(v as Var);
                }
            }
        }
        self.qhead = self.trail.len();
        if self.heur == Heur::Vmtf {
            self.vm_cursor = self.vm_head;
        }
    }

    // ----- main loop -----

    pub fn solve(&mut self) -> Verdict {
        if self.root_conflict {
            self.proof_add(&[]);
            return Verdict::Unsat;
        }
        if self.propagate().is_some() {
            self.proof_add(&[]);
            return Verdict::Unsat;
        }
        let mut restart_limit = 64 * luby(self.restart_num);
        loop {
            match self.propagate() {
                Some(confl) => {
                    self.conflicts += 1;
                    self.conflicts_since_restart += 1;
                    if self.trail_lim.is_empty() {
                        self.proof_add(&[]);
                        return Verdict::Unsat;
                    }
                    let (learned, bt, lbd) = self.analyze(confl);
                    self.proof_add(&learned);
                    self.cancel_until(bt);
                    if learned.len() == 1 {
                        self.enqueue(learned[0], None);
                    } else {
                        let cr = self.attach(Clause {
                            lits: learned,
                            learned: true,
                            deleted: false,
                            lbd,
                        });
                        self.learned_live += 1;
                        let assert_lit = self.clauses[cr].lits[0];
                        self.enqueue(assert_lit, Some(cr));
                    }
                    self.decay();

                    if self.conflicts % 1024 == 0 {
                        if let Some(d) = self.deadline {
                            if Instant::now() >= d {
                                return Verdict::Unknown;
                            }
                        }
                    }
                    if self.learned_live >= self.reduce_limit {
                        self.reduce_db();
                    }
                    if self.conflicts_since_restart >= restart_limit {
                        self.conflicts_since_restart = 0;
                        self.restart_num += 1;
                        self.stats_restarts += 1;
                        restart_limit = 64 * luby(self.restart_num);
                        self.cancel_until(0);
                    }
                }
                None => match self.pick_branch_var() {
                    None => {
                        let model = self.assign.iter().map(|&a| a == Val::True).collect();
                        return Verdict::Sat(model);
                    }
                    Some(v) => {
                        self.trail_lim.push(self.trail.len());
                        let phase_neg = !self.saved_phase[v as usize];
                        self.enqueue(lit(v, phase_neg), None);
                    }
                },
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
    fn luby_sequence() {
        let want = [1u64, 1, 2, 1, 1, 2, 4, 1, 1, 2, 1, 1, 2, 4, 8];
        for (i, &w) in want.iter().enumerate() {
            assert_eq!(luby(i as u64 + 1), w, "luby({})", i + 1);
        }
    }

    #[test]
    fn hand_cases() {
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
    fn both_heuristics_agree_with_dpll_on_random_3sat() {
        let mut state = 0x243F6A8885A308D3u64;
        let mut rng = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..500 {
            let n = 3 + (rng() % 10) as usize;
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
            let expected = dpll::solve(&f);
            for heur in [Heur::Evsids, Heur::Vmtf] {
                let v = Solver::with_config(&f, false, heur, None).solve();
                match (&v, &expected) {
                    (Verdict::Sat(m1), Verdict::Sat(_)) => check_model(&f, m1),
                    (Verdict::Unsat, Verdict::Unsat) => {}
                    _ => panic!(
                        "divergence round {round} heur {heur:?}: cdcl={v:?} dpll={expected:?} f={f:?}"
                    ),
                }
            }
        }
    }
}
