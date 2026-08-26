//! CLI. Output and exit codes follow minisat conventions so the M2
//! differential harness can compare verdicts directly:
//!   SAT   -> "s SATISFIABLE"   + "v ..." model line, exit code 10
//!   UNSAT -> "s UNSATISFIABLE",                      exit code 20
//! Errors -> exit code 1.

use std::process::ExitCode;
use unsat_with_receipts::{cdcl, dimacs, Verdict};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
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
    match cdcl::solve(&formula) {
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
            println!("s UNSATISFIABLE");
            ExitCode::from(20)
        }
    }
}
