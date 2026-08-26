# unsat-with-receipts

A proof-carrying CDCL SAT solver in Rust with zero dependencies. Every
UNSAT verdict ships a DRAT certificate that the independent checker
[drat-trim](https://github.com/marijnheule/drat-trim) verifies.

The claim under test: agent fleets can build infrastructure that proves
its own answers.

## Usage

```
unsat_with_receipts input.cnf [--proof out.drat]
```

Output and exit codes follow minisat conventions: `s SATISFIABLE` with a
`v` model line and exit 10, or `s UNSATISFIABLE` and exit 20. With
`--proof`, an UNSAT run writes a DRAT certificate.

## Verification status

| Gate | Result |
|------|--------|
| M2: verdict agreement with minisat on 10,000 fuzzed instances | 10,000/10,000 (6151 SAT / 3849 UNSAT) |
| M3: drat-trim verifies every UNSAT proof in the fuzz corpus | 3849/3849 `s VERIFIED` |
| M4 re-gate, EVSIDS: verdicts + proofs (with `d` deletion lines) | 10,000/10,000 agreement; 3849/3849 verified |
| M4 re-gate, VMTF (seed 43) | 10,000/10,000 agreement; 3870/3870 verified |

The fuzz corpus mixes phase-transition 3-SAT (ratio 4.26), off-ratio
3-SAT, 2-SAT and 4-SAT, pigeonhole, and random graph coloring
(deterministic xorshift, seed 42). SAT models are self-checked against
the formula. Reproduce with:

```
cargo run --bin fuzz -- 10000 fuzz-corpus 42
bash scripts/compare.sh fuzz-corpus       # minisat verdicts (WSL/Linux)
bash scripts/check-proofs.sh fuzz-corpus  # drat-trim over every proof
```

## Design

- CDCL: two-watched-literal propagation, 1UIP learning, backjumping.
- Every derived-clause addition or deletion goes through one proof choke
  point (`proof_add` / `proof_delete` in `src/cdcl.rs`); DRAT emission is
  built in, not retrofitted.
- `src/bin/shrink.rs` is a ddmin delta-shrinker that files minimal repros
  for any verdict divergence, with minisat in the loop.
- Heuristics: EVSIDS (default) or VMTF via `--heur`, Luby restarts
  (base 64), LBD-tiered deletion (glue <= 2 kept forever), phase saving.

## Benchmarks (SATLIB uniform random 3-SAT, 10 s per instance)

Full uf/uuf series, 6399 instances. Timeouts / instances, average seconds:

| Series | EVSIDS | VMTF |
|--------|--------|------|
| uf20..uf175 (2400 inst) | 0 timeouts, <= 0.053s avg | 0 timeouts, <= 0.275s avg |
| uf200-860 (100) | 0, 0.191s | 1, 0.791s |
| uf225-960 (100) | 0, 0.632s | 12, 2.036s |
| uf250-1065 (100) | 1, 2.112s | 21, 3.542s |
| uuf50..uuf150 (2300 inst) | 0 timeouts, <= 0.024s avg | 0 timeouts, <= 1.489s avg |
| uuf175-753 (100) | 0, 0.079s | 81, 9.330s |
| uuf200-860 (99) | 0, 0.315s | 99, 10.02s |
| uuf225-960 (100) | 0, 1.318s | 100, 10.02s |
| uuf250-1065 (100) | 24, 6.524s | 100, 10.02s |
| **Total timeouts** | **25 / 6399** | **414 / 6399** |

EVSIDS wins the A/B decisively: VMTF collapses on the larger UNSAT
series while staying competitive on satisfiable instances. Every verdict
matched the uf (SAT) / uuf (UNSAT) ground truth; SAT models were checked
against the formula. (SATLIB ships 99 files for uuf200-860, not 100.)

Structured 60 s tier (SATLIB DIMACS sets: par16, hanoi, hole, aim):
EVSIDS solves 9/10 (hole9 UNSAT in 9.6 s; hole10 times out — pigeonhole
is exponential for resolution). VMTF solves 8/10. `dubois*.cnf` was
excluded: the SATLIB files are malformed (header says 800 clauses, only
598 are `0`-terminated; minisat reads them the same wrong way).
