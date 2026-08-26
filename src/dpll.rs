//! Recursive DPLL with unit propagation.
//! ponytail: disposable M1 scaffolding — M2 replaces this with CDCL.
//! No watched literals, no heuristics: picks the first unassigned variable.

use crate::{Formula, Lit, Verdict};

pub fn solve(f: &Formula) -> Verdict {
    // assign[v] : 0 = unassigned, 1 = true, -1 = false (variable v+1).
    let mut assign = vec![0i8; f.num_vars];
    if f.clauses.iter().any(|c| c.is_empty()) {
        return Verdict::Unsat;
    }
    if dpll(&f.clauses, &mut assign) {
        // Unconstrained variables default to true.
        Verdict::Sat(assign.iter().map(|&a| a >= 0).collect())
    } else {
        Verdict::Unsat
    }
}

fn value(assign: &[i8], lit: Lit) -> i8 {
    let a = assign[lit.unsigned_abs() as usize - 1];
    if lit > 0 {
        a
    } else {
        -a
    }
}

/// Returns true if satisfiable under the current partial assignment.
fn dpll(clauses: &[Vec<Lit>], assign: &mut [i8]) -> bool {
    // Unit propagation to fixpoint.
    let mut trail: Vec<Lit> = Vec::new();
    loop {
        let mut propagated = false;
        for clause in clauses {
            let mut unassigned: Option<Lit> = None;
            let mut n_unassigned = 0;
            let mut satisfied = false;
            for &lit in clause {
                match value(assign, lit) {
                    1 => {
                        satisfied = true;
                        break;
                    }
                    0 => {
                        n_unassigned += 1;
                        unassigned = Some(lit);
                    }
                    _ => {}
                }
            }
            if satisfied {
                continue;
            }
            match n_unassigned {
                0 => {
                    // Conflict: undo propagations, fail.
                    for l in trail {
                        assign[l.unsigned_abs() as usize - 1] = 0;
                    }
                    return false;
                }
                1 => {
                    let lit = unassigned.unwrap();
                    assign[lit.unsigned_abs() as usize - 1] = if lit > 0 { 1 } else { -1 };
                    trail.push(lit);
                    propagated = true;
                }
                _ => {}
            }
        }
        if !propagated {
            break;
        }
    }

    // Pick first unassigned variable.
    match assign.iter().position(|&a| a == 0) {
        None => true, // all assigned, no conflict => satisfied
        Some(v) => {
            for val in [1i8, -1] {
                assign[v] = val;
                if dpll(clauses, assign) {
                    return true;
                }
            }
            assign[v] = 0;
            for l in trail {
                assign[l.unsigned_abs() as usize - 1] = 0;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dimacs::parse;

    fn check_model(f: &Formula, model: &[bool]) {
        for clause in &f.clauses {
            assert!(
                clause
                    .iter()
                    .any(|&l| model[l.unsigned_abs() as usize - 1] == (l > 0)),
                "model does not satisfy clause {clause:?}"
            );
        }
    }

    fn run(input: &str) -> (Formula, Verdict) {
        let f = parse(input).unwrap();
        let v = solve(&f);
        (f, v)
    }

    #[test]
    fn empty_formula_is_sat() {
        let (_, v) = run("p cnf 0 0\n");
        assert!(matches!(v, Verdict::Sat(_)));
    }

    #[test]
    fn empty_clause_is_unsat() {
        let (_, v) = run("p cnf 1 1\n0\n");
        assert_eq!(v, Verdict::Unsat);
    }

    #[test]
    fn unit_chain() {
        // 1, 1->2, 2->3 : forces all true.
        let (f, v) = run("1 0\n-1 2 0\n-2 3 0\n");
        match v {
            Verdict::Sat(m) => {
                assert_eq!(m, vec![true, true, true]);
                check_model(&f, &m);
            }
            _ => panic!("expected SAT"),
        }
    }

    #[test]
    fn trivially_sat_three_clauses() {
        let (f, v) = run("1 2 0\n-1 3 0\n-3 -2 1 0\n");
        match v {
            Verdict::Sat(m) => check_model(&f, &m),
            _ => panic!("expected SAT"),
        }
    }

    #[test]
    fn contradiction_is_unsat() {
        let (_, v) = run("1 0\n-1 0\n");
        assert_eq!(v, Verdict::Unsat);
    }

    #[test]
    fn php_2_pigeons_1_hole() {
        // Pigeons 1,2 into hole A: x1=p1-in-A, x2=p2-in-A.
        let (_, v) = run("1 0\n2 0\n-1 -2 0\n");
        assert_eq!(v, Verdict::Unsat);
    }

    #[test]
    fn php_3_pigeons_2_holes() {
        // x_{p,h}, p in 1..3, h in 1..2; var = 2(p-1)+h.
        let (_, v) = run(
            "1 2 0\n3 4 0\n5 6 0\n\
             -1 -3 0\n-1 -5 0\n-3 -5 0\n\
             -2 -4 0\n-2 -6 0\n-4 -6 0\n",
        );
        assert_eq!(v, Verdict::Unsat);
    }

    #[test]
    fn model_covers_unconstrained_vars() {
        // Var 2 never appears; model must still have an entry for it.
        let (f, v) = run("p cnf 3 1\n1 3 0\n");
        match v {
            Verdict::Sat(m) => {
                assert_eq!(m.len(), 3);
                check_model(&f, &m);
            }
            _ => panic!("expected SAT"),
        }
    }
}
