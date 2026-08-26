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
- M4 (in progress): EVSIDS/VMTF decision heuristics behind a flag, Luby
  restarts, LBD-tiered clause deletion, SATLIB benchmark runs.
