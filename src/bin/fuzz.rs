//! Differential fuzz corpus generator + our-side solver.
//!
//! Usage: fuzz <count> <out_dir> [seed] [evsids|vmtf]
//! Writes <out_dir>/<i>.cnf for each instance and <out_dir>/ours.txt with
//! lines "<i> SAT|UNSAT". A WSL script then runs minisat over the corpus
//! and the verdicts are compared.
//!
//! Generators (per settled spec): phase-transition k-SAT (ratio 4.26 for
//! k=3), pigeonhole, random graph coloring. Benchmark mutation joins at M4
//! when SATLIB instances are on disk.

use std::fmt::Write as _;
use unsat_with_receipts::{cdcl, Formula, Verdict};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn random_ksat(rng: &mut Rng, k: usize, n: usize, ratio: f64) -> Formula {
    let m = (n as f64 * ratio).round() as usize;
    let mut f = Formula {
        num_vars: n,
        clauses: Vec::new(),
    };
    for _ in 0..m {
        let mut c = Vec::new();
        for _ in 0..k {
            let v = rng.below(n as u64) as i32 + 1;
            c.push(if rng.next() & 1 == 0 { v } else { -v });
        }
        f.clauses.push(c);
    }
    f
}

/// PHP(p pigeons, h holes): UNSAT iff p > h.
fn pigeonhole(p: usize, h: usize) -> Formula {
    let var = |pi: usize, hi: usize| (pi * h + hi) as i32 + 1;
    let mut f = Formula {
        num_vars: p * h,
        clauses: Vec::new(),
    };
    for pi in 0..p {
        f.clauses.push((0..h).map(|hi| var(pi, hi)).collect());
    }
    for hi in 0..h {
        for a in 0..p {
            for b in a + 1..p {
                f.clauses.push(vec![-var(a, hi), -var(b, hi)]);
            }
        }
    }
    f
}

/// Random graph k-coloring: n nodes, edge probability ~edges/possible.
fn coloring(rng: &mut Rng, n: usize, k: usize, edge_pct: u64) -> Formula {
    let var = |node: usize, color: usize| (node * k + color) as i32 + 1;
    let mut f = Formula {
        num_vars: n * k,
        clauses: Vec::new(),
    };
    for node in 0..n {
        f.clauses.push((0..k).map(|c| var(node, c)).collect());
        for a in 0..k {
            for b in a + 1..k {
                f.clauses.push(vec![-var(node, a), -var(node, b)]);
            }
        }
    }
    for a in 0..n {
        for b in a + 1..n {
            if rng.below(100) < edge_pct {
                for c in 0..k {
                    f.clauses.push(vec![-var(a, c), -var(b, c)]);
                }
            }
        }
    }
    f
}

fn to_dimacs(f: &Formula) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "p cnf {} {}", f.num_vars, f.clauses.len());
    for c in &f.clauses {
        for l in c {
            let _ = write!(s, "{l} ");
        }
        let _ = writeln!(s, "0");
    }
    s
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let count: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10000);
    let dir = args.get(2).cloned().unwrap_or_else(|| "fuzz-corpus".into());
    let seed: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0x9E3779B97F4A7C15);
    let heur = match args.get(4).map(String::as_str) {
        Some("vmtf") => cdcl::Heur::Vmtf,
        _ => cdcl::Heur::Evsids,
    };
    let mut rng = Rng(seed);
    std::fs::create_dir_all(&dir).expect("create out dir");

    let mut ours = String::new();
    for i in 0..count {
        let f = match rng.below(10) {
            // 60% phase-transition 3-SAT, the hard zone.
            0..=5 => {
                let n = 5 + rng.below(45) as usize;
                random_ksat(&mut rng, 3, n, 4.26)
            }
            // Off-ratio 3-SAT so easy SAT/UNSAT are covered too.
            6 => {
                let n = 5 + rng.below(30) as usize;
                let ratio = 2.0 + rng.below(50) as f64 / 10.0; // 2.0..7.0
                random_ksat(&mut rng, 3, n, ratio)
            }
            // 2-SAT and 4-SAT at their rough transitions.
            7 => {
                let n = 5 + rng.below(40) as usize;
                random_ksat(&mut rng, 2, n, 1.0)
            }
            8 => {
                let n = 5 + rng.below(20) as usize;
                random_ksat(&mut rng, 4, n, 9.9)
            }
            // Structured: pigeonhole or coloring.
            _ => {
                if rng.next() & 1 == 0 {
                    let h = 1 + rng.below(4) as usize;
                    let p = h + rng.below(3) as usize; // h..h+2 pigeons
                    pigeonhole(p, h)
                } else {
                    let n = 4 + rng.below(8) as usize;
                    let k = 2 + rng.below(3) as usize;
                    let pct = 30 + rng.below(50);
                    coloring(&mut rng, n, k, pct)
                }
            }
        };
        std::fs::write(format!("{dir}/{i}.cnf"), to_dimacs(&f)).expect("write cnf");
        let mut solver = cdcl::Solver::with_config(&f, true, heur, None);
        let v = match solver.solve() {
            Verdict::Sat(model) => {
                // Self-check: the model must satisfy the formula.
                for c in &f.clauses {
                    assert!(
                        c.iter().any(|&l| model[l.unsigned_abs() as usize - 1] == (l > 0)),
                        "instance {i}: model does not satisfy {c:?}"
                    );
                }
                "SAT"
            }
            Verdict::Unsat => {
                let mut out = solver.proof.join("\n");
                out.push('\n');
                std::fs::write(format!("{dir}/{i}.drat"), out).expect("write drat");
                "UNSAT"
            }
            Verdict::Unknown => unreachable!("no timeout set in fuzz"),
        };
        let _ = writeln!(ours, "{i} {v}");
    }
    std::fs::write(format!("{dir}/ours.txt"), ours).expect("write ours.txt");
    println!("wrote {count} instances to {dir}");
}
