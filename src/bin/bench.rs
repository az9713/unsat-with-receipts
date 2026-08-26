//! SATLIB benchmark runner.
//!
//! Usage: bench <dir> <timeout_secs> <evsids|vmtf> <out.csv>
//! Recursively solves every .cnf under <dir> with the given per-instance
//! timeout. CSV: path,verdict,seconds,restarts,deleted. uf* files are
//! expected SAT, uuf* expected UNSAT; a mismatch aborts (that would be a
//! solver bug, not a benchmark result).

use std::time::{Duration, Instant};
use unsat_with_receipts::{cdcl, dimacs, Verdict};

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).expect("read_dir").flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "cnf") {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: bench <dir> <timeout_secs> <evsids|vmtf> <out.csv>");
        std::process::exit(1);
    }
    let timeout: f64 = args[2].parse().expect("timeout seconds");
    let heur = match args[3].as_str() {
        "evsids" => cdcl::Heur::Evsids,
        "vmtf" => cdcl::Heur::Vmtf,
        other => panic!("unknown heuristic {other}"),
    };
    let mut files = Vec::new();
    walk(std::path::Path::new(&args[1]), &mut files);
    files.sort();
    println!("{} instances, timeout {timeout}s, {heur:?}", files.len());

    let mut csv = String::from("path,verdict,seconds,restarts,deleted\n");
    let (mut sat, mut unsat, mut unknown) = (0u32, 0u32, 0u32);
    for (i, path) in files.iter().enumerate() {
        let input = std::fs::read_to_string(path).expect("read cnf");
        let f = dimacs::parse(&input).expect("parse cnf");
        let start = Instant::now();
        let deadline = start + Duration::from_secs_f64(timeout);
        let mut solver = cdcl::Solver::with_config(&f, false, heur, Some(deadline));
        let verdict = solver.solve();
        let secs = start.elapsed().as_secs_f64();
        let name = path.to_string_lossy().replace('\\', "/");
        let base = path.file_name().unwrap().to_string_lossy().to_string();
        let v = match verdict {
            Verdict::Sat(model) => {
                for c in &f.clauses {
                    assert!(
                        c.iter().any(|&l| model[l.unsigned_abs() as usize - 1] == (l > 0)),
                        "{name}: model does not satisfy {c:?}"
                    );
                }
                // Expected-verdict check applies only to uf*/uuf* names.
                assert!(
                    !base.starts_with("uuf"),
                    "{name}: SAT on an expected-UNSAT instance"
                );
                sat += 1;
                "SAT"
            }
            Verdict::Unsat => {
                assert!(
                    !base.starts_with("uf"),
                    "{name}: UNSAT on an expected-SAT instance"
                );
                unsat += 1;
                "UNSAT"
            }
            Verdict::Unknown => {
                unknown += 1;
                "UNKNOWN"
            }
        };
        csv.push_str(&format!(
            "{name},{v},{secs:.3},{},{}\n",
            solver.stats_restarts, solver.stats_deleted
        ));
        if (i + 1) % 500 == 0 {
            println!("{}/{} sat={sat} unsat={unsat} timeout={unknown}", i + 1, files.len());
        }
    }
    std::fs::write(&args[4], csv).expect("write csv");
    println!("done: sat={sat} unsat={unsat} timeout={unknown} -> {}", args[4]);
}
