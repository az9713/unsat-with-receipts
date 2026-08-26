//! M5: Sudoku encoder — and a uniqueness proof with a receipt.
//!
//! Encodes a 9x9 Sudoku as CNF, solves it, prints the grid. Then adds a
//! blocking clause forbidding that exact solution and re-solves with DRAT
//! on: UNSAT means the solution is UNIQUE, and the proof is the
//! machine-checkable receipt for that claim.
//!
//! Usage: sudoku [PUZZLE] [--proof FILE] [--dimacs FILE]
//! PUZZLE is 81 chars, digits 1-9 and '.'/'0' for blanks (row-major).

use std::process::ExitCode;
use unsat_with_receipts::{cdcl, Formula, Lit, Verdict};

// Wikipedia's standard example puzzle (unique solution).
const DEFAULT: &str = "53..7....6..195....98....6.8...6...34..8.3..17...2...6.6....28....419..5....8..79";

/// Variable for "cell (r,c) holds digit d" (all 0-based).
fn var(r: usize, c: usize, d: usize) -> Lit {
    (r * 81 + c * 9 + d + 1) as Lit
}

fn encode(puzzle: &[u8]) -> Formula {
    let mut f = Formula { num_vars: 729, clauses: Vec::new() };
    for r in 0..9 {
        for c in 0..9 {
            // Each cell: at least one digit, at most one digit.
            f.clauses.push((0..9).map(|d| var(r, c, d)).collect());
            for d1 in 0..9 {
                for d2 in d1 + 1..9 {
                    f.clauses.push(vec![-var(r, c, d1), -var(r, c, d2)]);
                }
            }
        }
    }
    // Each digit at most once per row, column, and 3x3 box.
    for d in 0..9 {
        for i in 0..9 {
            for j1 in 0..9 {
                for j2 in j1 + 1..9 {
                    f.clauses.push(vec![-var(i, j1, d), -var(i, j2, d)]);
                    f.clauses.push(vec![-var(j1, i, d), -var(j2, i, d)]);
                }
            }
        }
        for b in 0..9 {
            let (br, bc) = (b / 3 * 3, b % 3 * 3);
            let cells: Vec<(usize, usize)> =
                (0..9).map(|k| (br + k / 3, bc + k % 3)).collect();
            for i in 0..9 {
                for j in i + 1..9 {
                    let (r1, c1) = cells[i];
                    let (r2, c2) = cells[j];
                    f.clauses.push(vec![-var(r1, c1, d), -var(r2, c2, d)]);
                }
            }
        }
    }
    // Givens.
    for (i, &ch) in puzzle.iter().enumerate() {
        if ch.is_ascii_digit() && ch != b'0' {
            f.clauses.push(vec![var(i / 9, i % 9, (ch - b'1') as usize)]);
        }
    }
    f
}

fn write_dimacs(f: &Formula, path: &str) {
    let mut s = format!("p cnf {} {}\n", f.num_vars, f.clauses.len());
    for cl in &f.clauses {
        for l in cl {
            s.push_str(&l.to_string());
            s.push(' ');
        }
        s.push_str("0\n");
    }
    std::fs::write(path, s).expect("write dimacs");
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut take_opt = |name: &str| -> Option<String> {
        let pos = args.iter().position(|a| a == name)?;
        let v = args.remove(pos + 1);
        args.remove(pos);
        Some(v)
    };
    let proof_path = take_opt("--proof");
    let dimacs_path = take_opt("--dimacs");
    let puzzle = args.pop().unwrap_or_else(|| DEFAULT.to_string());
    let puzzle = puzzle.into_bytes();
    if puzzle.len() != 81 {
        eprintln!("puzzle must be 81 chars, got {}", puzzle.len());
        return ExitCode::from(1);
    }

    // Solve.
    let f = encode(&puzzle);
    let model = match cdcl::Solver::new(&f, false).solve() {
        Verdict::Sat(m) => m,
        _ => {
            println!("s UNSATISFIABLE (puzzle has no solution)");
            return ExitCode::from(20);
        }
    };
    let mut solution = [[0usize; 9]; 9];
    for r in 0..9 {
        for c in 0..9 {
            for d in 0..9 {
                if model[(var(r, c, d) - 1) as usize] {
                    solution[r][c] = d + 1;
                }
            }
        }
    }
    println!("c solved:");
    for row in &solution {
        println!("c   {}", row.map(|d| d.to_string()).join(" "));
    }

    // Uniqueness: block this solution, expect UNSAT with a receipt.
    let mut f2 = f;
    f2.clauses.push(
        (0..81).map(|i| -var(i / 9, i % 9, solution[i / 9][i % 9] - 1)).collect(),
    );
    if let Some(path) = &dimacs_path {
        write_dimacs(&f2, path);
    }
    let mut solver = cdcl::Solver::new(&f2, proof_path.is_some());
    match solver.solve() {
        Verdict::Unsat => {
            if let Some(path) = &proof_path {
                let mut out = solver.proof.join("\n");
                out.push('\n');
                std::fs::write(path, out).expect("write proof");
                println!("c DRAT uniqueness receipt written to {path}");
            }
            println!("s UNSATISFIABLE (blocked) — solution is UNIQUE");
            ExitCode::from(20)
        }
        Verdict::Sat(_) => {
            println!("s SATISFIABLE (blocked) — puzzle has MULTIPLE solutions");
            ExitCode::from(10)
        }
        Verdict::Unknown => {
            println!("s UNKNOWN");
            ExitCode::from(0)
        }
    }
}
