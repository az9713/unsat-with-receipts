//! CLI. Output and exit codes follow minisat conventions so the M2
//! differential harness can compare verdicts directly:
//!   SAT   -> "s SATISFIABLE"   + "v ..." model line, exit code 10
//!   UNSAT -> "s UNSATISFIABLE",                      exit code 20
//! Errors -> exit code 1.

use std::process::ExitCode;
use unsat_with_receipts::{cdcl, dimacs, Verdict};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().collect();
    // --proof <file>: write a DRAT certificate on UNSAT.
    let mut proof_path: Option<String> = None;
    if let Some(pos) = args.iter().position(|a| a == "--proof") {
        if pos + 1 >= args.len() {
            eprintln!("--proof needs a file argument");
            return ExitCode::from(1);
        }
        proof_path = Some(args.remove(pos + 1));
        args.remove(pos);
    }
    let input = match args.get(1) {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error reading {path}: {e}");
                return ExitCode::from(1);
            }
        },
        None => {
            let mut s = String::new();
            use std::io::Read;
            if let Err(e) = std::io::stdin().read_to_string(&mut s) {
                eprintln!("error reading stdin: {e}");
                return ExitCode::from(1);
            }
            s
        }
    };
    let formula = match dimacs::parse(&input) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let mut solver = cdcl::Solver::new(&formula, proof_path.is_some());
    match solver.solve() {
        Verdict::Sat(model) => {
            println!("s SATISFIABLE");
            let lits: Vec<String> = model
                .iter()
                .enumerate()
                .map(|(i, &b)| {
                    let v = (i + 1) as i64;
                    (if b { v } else { -v }).to_string()
                })
                .collect();
            println!("v {} 0", lits.join(" "));
            ExitCode::from(10)
        }
        Verdict::Unsat => {
            if let Some(path) = proof_path {
                let mut out = solver.proof.join("\n");
                out.push('\n');
                if let Err(e) = std::fs::write(&path, out) {
                    eprintln!("error writing proof {path}: {e}");
                    return ExitCode::from(1);
                }
            }
            println!("s UNSATISFIABLE");
            ExitCode::from(20)
        }
    }
}
