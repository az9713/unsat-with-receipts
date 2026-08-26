//! Delta-shrinker for divergent instances.
//!
//! Usage: shrink <input.cnf> <minisat_cmd> [out.cnf]
//! Repeatedly removes clauses (chunks, then singles) while OUR verdict
//! still differs from minisat's on the shrunk instance. minisat_cmd is a
//! command run as `<minisat_cmd> <file>`; exit 10 = SAT, 20 = UNSAT
//! (e.g. "wsl minisat -verb=0" from Windows). Writes the minimal repro.

use std::process::Command;
use unsat_with_receipts::{cdcl, dimacs, Formula, Verdict};

fn ours(f: &Formula) -> &'static str {
    match cdcl::solve(f) {
        Verdict::Sat(_) => "SAT",
        Verdict::Unsat => "UNSAT",
        Verdict::Unknown => unreachable!("no timeout set in shrink"),
    }
}

fn theirs(cmd: &[String], f: &Formula, tmp: &str) -> Option<&'static str> {
    std::fs::write(tmp, to_dimacs(f)).ok()?;
    let status = Command::new(&cmd[0]).args(&cmd[1..]).arg(tmp).output().ok()?;
    match status.status.code() {
        Some(10) => Some("SAT"),
        Some(20) => Some("UNSAT"),
        _ => None,
    }
}

fn to_dimacs(f: &Formula) -> String {
    let mut s = format!("p cnf {} {}\n", f.num_vars, f.clauses.len());
    for c in &f.clauses {
        for l in c {
            s.push_str(&l.to_string());
            s.push(' ');
        }
        s.push_str("0\n");
    }
    s
}

fn diverges(cmd: &[String], f: &Formula, tmp: &str) -> bool {
    match theirs(cmd, f, tmp) {
        Some(t) => t != ours(f),
        None => false,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: shrink <input.cnf> <minisat_cmd...> [-- out.cnf]");
        std::process::exit(1);
    }
    let input = std::fs::read_to_string(&args[1]).expect("read input");
    let mut f = dimacs::parse(&input).expect("parse input");
    let cmd: Vec<String> = args[2].split_whitespace().map(String::from).collect();
    let out = args.get(3).cloned().unwrap_or_else(|| "repro-min.cnf".into());
    let tmp = "shrink-tmp.cnf";

    assert!(
        diverges(&cmd, &f, tmp),
        "input does not diverge; nothing to shrink"
    );

    // ddmin over clauses: try removing chunks of decreasing size.
    let mut chunk = f.clauses.len().div_ceil(2).max(1);
    while chunk >= 1 {
        let mut i = 0;
        let mut removed_any = false;
        while i < f.clauses.len() {
            let end = (i + chunk).min(f.clauses.len());
            let mut candidate = f.clone();
            candidate.clauses.drain(i..end);
            if diverges(&cmd, &candidate, tmp) {
                f = candidate;
                removed_any = true;
                // keep i: next chunk shifted into place
            } else {
                i = end;
            }
        }
        if chunk == 1 && !removed_any {
            break;
        }
        if !removed_any {
            chunk /= 2;
        }
    }

    // Literal-level pass: try dropping single literals from clauses.
    let mut changed = true;
    while changed {
        changed = false;
        for ci in 0..f.clauses.len() {
            let mut li = 0;
            while li < f.clauses[ci].len() {
                let mut candidate = f.clone();
                candidate.clauses[ci].remove(li);
                if diverges(&cmd, &candidate, tmp) {
                    f = candidate;
                    changed = true;
                } else {
                    li += 1;
                }
            }
        }
    }

    let _ = std::fs::remove_file(tmp);
    std::fs::write(&out, to_dimacs(&f)).expect("write output");
    println!(
        "minimal repro: {} clauses, {} vars -> {out}",
        f.clauses.len(),
        f.num_vars
    );
}
