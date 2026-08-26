//! M5: circuit-equivalence miter demo.
//!
//! Builds two n-bit adders — a ripple-carry adder (majority-gate carries)
//! and a carry-lookahead-style adder (generate/propagate) — over shared
//! inputs, Tseitin-encodes both, XORs corresponding outputs, and asserts
//! that some output differs. UNSAT means the circuits are equivalent, and
//! the DRAT proof is the machine-checkable receipt.
//!
//! `--buggy` plants a carry bug in the lookahead adder (drops the
//! propagate term at one bit). The miter then goes SAT and the model
//! decodes to a concrete counterexample input.
//!
//! Usage: miter [--bits N] [--buggy] [--proof FILE]

use std::process::ExitCode;
use unsat_with_receipts::{cdcl, Formula, Lit, Verdict};

/// Tseitin gate builder: allocates fresh variables, appends clauses.
struct Circuit {
    f: Formula,
}

impl Circuit {
    fn fresh(&mut self) -> Lit {
        self.f.num_vars += 1;
        self.f.num_vars as Lit
    }
    fn clause(&mut self, lits: &[Lit]) {
        self.f.clauses.push(lits.to_vec());
    }
    /// o <-> a XOR b
    fn xor2(&mut self, a: Lit, b: Lit) -> Lit {
        let o = self.fresh();
        self.clause(&[-a, -b, -o]);
        self.clause(&[a, b, -o]);
        self.clause(&[a, -b, o]);
        self.clause(&[-a, b, o]);
        o
    }
    /// o <-> a AND b
    fn and2(&mut self, a: Lit, b: Lit) -> Lit {
        let o = self.fresh();
        self.clause(&[-a, -b, o]);
        self.clause(&[a, -o]);
        self.clause(&[b, -o]);
        o
    }
    /// o <-> a OR b
    fn or2(&mut self, a: Lit, b: Lit) -> Lit {
        let o = self.fresh();
        self.clause(&[a, b, -o]);
        self.clause(&[-a, o]);
        self.clause(&[-b, o]);
        o
    }
    /// o <-> majority(a, b, c)
    fn maj3(&mut self, a: Lit, b: Lit, c: Lit) -> Lit {
        let o = self.fresh();
        self.clause(&[-a, -b, o]);
        self.clause(&[-a, -c, o]);
        self.clause(&[-b, -c, o]);
        self.clause(&[a, b, -o]);
        self.clause(&[a, c, -o]);
        self.clause(&[b, c, -o]);
        o
    }
}

/// Ripple-carry adder: s_i = a^b^c, carry via majority gate.
/// Returns n sum bits plus the carry-out.
fn ripple_carry(c: &mut Circuit, a: &[Lit], b: &[Lit], cin: Lit) -> Vec<Lit> {
    let mut outs = Vec::new();
    let mut carry = cin;
    for i in 0..a.len() {
        let ab = c.xor2(a[i], b[i]);
        outs.push(c.xor2(ab, carry));
        carry = c.maj3(a[i], b[i], carry);
    }
    outs.push(carry);
    outs
}

/// Lookahead-style adder: p = a^b, g = a&b, carry = g | (p & c).
/// `bug_at` (for --buggy) drops the propagate term at that bit:
/// carry = g | c, which over-propagates carries.
fn lookahead(c: &mut Circuit, a: &[Lit], b: &[Lit], cin: Lit, bug_at: Option<usize>) -> Vec<Lit> {
    let mut outs = Vec::new();
    let mut carry = cin;
    for i in 0..a.len() {
        let p = c.xor2(a[i], b[i]);
        let g = c.and2(a[i], b[i]);
        outs.push(c.xor2(p, carry));
        carry = if bug_at == Some(i) {
            c.or2(g, carry)
        } else {
            let pc = c.and2(p, carry);
            c.or2(g, pc)
        };
    }
    outs.push(carry);
    outs
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take_flag = |name: &str| -> bool {
        args.iter().position(|a| a == name).map(|p| args.remove(p)).is_some()
    };
    let buggy = take_flag("--buggy");
    let mut take_opt = |name: &str| -> Option<String> {
        let pos = args.iter().position(|a| a == name)?;
        let v = args.remove(pos + 1);
        args.remove(pos);
        Some(v)
    };
    let bits: usize = take_opt("--bits").map_or(8, |s| s.parse().expect("bad --bits"));
    let proof_path = take_opt("--proof");
    let dimacs_path = take_opt("--dimacs");

    let mut c = Circuit { f: Formula::default() };
    let a: Vec<Lit> = (0..bits).map(|_| c.fresh()).collect();
    let b: Vec<Lit> = (0..bits).map(|_| c.fresh()).collect();
    let cin = c.fresh();

    let out1 = ripple_carry(&mut c, &a, &b, cin);
    let bug_at = if buggy { Some(bits / 2) } else { None };
    let out2 = lookahead(&mut c, &a, &b, cin, bug_at);

    // Miter: some pair of corresponding outputs differs.
    let diffs: Vec<Lit> = out1.iter().zip(&out2).map(|(&x, &y)| c.xor2(x, y)).collect();
    c.clause(&diffs);

    println!(
        "c miter: {bits}-bit ripple-carry vs lookahead{}, {} vars, {} clauses",
        if buggy { " (bug planted)" } else { "" },
        c.f.num_vars,
        c.f.clauses.len()
    );

    if let Some(path) = &dimacs_path {
        let mut s = format!("p cnf {} {}\n", c.f.num_vars, c.f.clauses.len());
        for cl in &c.f.clauses {
            for l in cl {
                s.push_str(&l.to_string());
                s.push(' ');
            }
            s.push_str("0\n");
        }
        std::fs::write(path, s).expect("write dimacs");
    }

    let mut solver = cdcl::Solver::new(&c.f, proof_path.is_some());
    match solver.solve() {
        Verdict::Unsat => {
            if let Some(path) = &proof_path {
                let mut out = solver.proof.join("\n");
                out.push('\n');
                std::fs::write(path, out).expect("write proof");
                println!("c DRAT proof written to {path}");
            }
            println!("s UNSATISFIABLE");
            println!("c circuits are EQUIVALENT (miter has no distinguishing input)");
            ExitCode::from(20)
        }
        Verdict::Sat(model) => {
            let val = |lits: &[Lit]| -> u64 {
                lits.iter()
                    .enumerate()
                    .map(|(i, &l)| (model[(l - 1) as usize] as u64) << i)
                    .sum()
            };
            let (av, bv, ci) = (val(&a), val(&b), val(&[cin]));
            println!("s SATISFIABLE");
            println!("c circuits DIFFER; counterexample: a={av} b={bv} cin={ci}");
            println!(
                "c ripple-carry says {}, lookahead says {} (true sum {})",
                val(&out1),
                val(&out2),
                av + bv + ci
            );
            ExitCode::from(10)
        }
        Verdict::Unknown => {
            println!("s UNKNOWN");
            ExitCode::from(0)
        }
    }
}
