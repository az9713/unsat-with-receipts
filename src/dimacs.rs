//! Lenient DIMACS CNF parser.
//!
//! Lenient the way minisat is, on purpose: the M2 differential fuzzer must
//! not report parser strictness differences as solver divergences.
//! - `c` comment lines are skipped anywhere.
//! - The `p cnf <vars> <clauses>` header is optional; counts in it are
//!   advisory (we grow past them instead of erroring).
//! - Clauses are 0-terminated token streams; they may span lines or share one.
//! - `%` (SATLIB trailer) ends the input.

use crate::{Formula, Lit};

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error: {}", self.0)
    }
}

pub fn parse(input: &str) -> Result<Formula, ParseError> {
    let mut f = Formula::default();
    let mut cur: Vec<Lit> = Vec::new();
    for line in input.lines() {
        let line = line.trim();
        if line.starts_with('c') || line.starts_with('p') {
            continue;
        }
        if line.starts_with('%') {
            break;
        }
        for tok in line.split_ascii_whitespace() {
            let lit: i64 = tok
                .parse()
                .map_err(|_| ParseError(format!("bad token {tok:?}")))?;
            if lit == 0 {
                f.clauses.push(std::mem::take(&mut cur));
            } else {
                if lit.unsigned_abs() > i32::MAX as u64 {
                    return Err(ParseError(format!("literal out of range: {lit}")));
                }
                f.num_vars = f.num_vars.max(lit.unsigned_abs() as usize);
                cur.push(lit as Lit);
            }
        }
    }
    if !cur.is_empty() {
        // Trailing clause without terminating 0: accept it, like minisat.
        f.clauses.push(cur);
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let f = parse("c comment\np cnf 3 2\n1 -2 0\n2 3 0\n").unwrap();
        assert_eq!(f.num_vars, 3);
        assert_eq!(f.clauses, vec![vec![1, -2], vec![2, 3]]);
    }

    #[test]
    fn clauses_span_and_share_lines() {
        let f = parse("1 -2\n3 0 2 0\n").unwrap();
        assert_eq!(f.clauses, vec![vec![1, -2, 3], vec![2]]);
    }

    #[test]
    fn grows_past_header() {
        let f = parse("p cnf 1 1\n5 0\n").unwrap();
        assert_eq!(f.num_vars, 5);
    }

    #[test]
    fn empty_clause_and_satlib_trailer() {
        let f = parse("0\n%\n99 0\n").unwrap();
        assert_eq!(f.clauses, vec![Vec::<Lit>::new()]);
    }

    #[test]
    fn trailing_unterminated_clause() {
        let f = parse("1 2 3").unwrap();
        assert_eq!(f.clauses, vec![vec![1, 2, 3]]);
    }

    #[test]
    fn bad_token_is_error() {
        assert!(parse("1 x 0").is_err());
    }
}
