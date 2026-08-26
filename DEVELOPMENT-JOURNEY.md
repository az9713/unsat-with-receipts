# Development Journey — "unsat-with-receipts" (2026-08-25/26)

**Date:** 2026-08-26
**Deliverable:** https://github.com/az9713/unsat-with-receipts — a proof-carrying CDCL SAT solver in Rust, zero dependencies
**Brief:** build a SAT solver whose UNSAT answers ship machine-checkable DRAT receipts, gated milestone by milestone against independent oracles (minisat, drat-trim).
**Evidence page:** `docs/index.html` (GitHub Pages)

**Provenance warning.** This document was written after a `/clear`. Sections
on M1–M4 are **reconstructed** from `HANDOFF.md`, the five commit messages,
`README.md`, and the committed benchmark CSVs — not from the live transcript.
The M5 section is written from the live session that produced it. Where the
sources are silent (exact error text from M1–M4, dead ends inside those
milestones), this document says nothing rather than inventing detail.
Revised once after the M5 commit, at Simon's request: framing trimmed,
the decision and wart lists expanded from the still-live M5 transcript.

## 1. The brief — infrastructure that proves its own answers

The thesis line, from the README: "agent fleets can build infrastructure that
proves its own answers." A SAT verdict is easy to trust; a model checks in
linear time. UNSAT is a universal claim — *no* assignment works — so the
solver emits a DRAT proof for every UNSAT verdict and an independent checker
(drat-trim, by Marijn Heule) verifies it. The spec was settled up front by a
15-question grilling session (the `grilling` skill); the settled answers live
in the README and commit messages, and HANDOFF.md marks them "do not
re-litigate."

Five milestones, each with a hard gate before its commit:

- **M1** `efef2af` — lenient DIMACS parser + recursive DPLL, minisat-compatible
  CLI (exit 10/20, `s` lines). Gate: 14 unit tests + hand cases vs minisat.
- **M2** `f1057e7` — CDCL (two-watched literals, 1UIP, backjumping) + a
  differential fuzz harness. Gate: 10,000/10,000 verdict agreement with
  minisat (6151 SAT / 3849 UNSAT, seed 42), SAT models self-checked.
- **M3** `7e2a3bd` — DRAT emission via `--proof`. Gate: 3849/3849 fuzz-corpus
  proofs `s VERIFIED` by drat-trim. First public push happened here, per spec.
- **M4** `e9c9451` — EVSIDS + VMTF heuristics, Luby restarts, LBD-tiered
  deletion, phase saving, `--timeout`. Gates re-passed for both heuristics;
  full SATLIB sweep.
- **M5** (this session) — circuit-equivalence miter, Sudoku uniqueness
  receipt, evidence page, this document.

## 2. Cold start — resuming after /clear

Minute zero of the M5 session: read `HANDOFF.md` (the repo's resume file,
written by the `handoff-after-clear` skill in the prior session), confirm the
repo state with `git log --oneline`, and inspect the library surface
(`src/lib.rs`, the `Solver::with_config` API, how `main.rs` writes
`solver.proof`). Nothing had to be rebuilt or re-verified; everything
load-bearing was committed, and the HANDOFF's environment facts held on first
try. An advisor consultation before any code locked the M5 work order
(miter first — the only piece with a hard external gate) and added two things
the HANDOFF did not say: give the miter a **negative control**, and shape the
Sudoku demo as a **uniqueness proof** rather than a plain encoder. Both
survived into the deliverable.

## 3. Design decisions

Reconstructed decisions (M1–M4), with the losing options where the record
keeps them:

- **One proof choke point.** Every derived-clause add/delete routes through
  `proof_add` / `proof_delete` in `src/cdcl.rs`. DRAT was built in at M2 —
  the lines just stayed empty until M3 — rather than retrofitted. This is the
  project's central design bet and it paid: the M4 deletion machinery (LBD
  tiers, tombstones, lazy watch cleanup) emitted correct `d` lines through
  the same two functions.
- **Zero dependencies.** No CLI crate, no rand crate (deterministic xorshift,
  seed 42), no test framework beyond `#[test]`.
- **Minisat CLI conventions** (exit 10/20, `s`/`v` lines) so the differential
  harness compares verdicts with no adapter.
- **Disposable DPLL.** M1's recursive DPLL was labeled scaffolding in its own
  doc comment and replaced by CDCL in M2; the parser and CLI survived.
- **EVSIDS as default** after a real A/B: VMTF collapses on larger UNSAT
  series (414 vs 25 timeouts over 6399 SATLIB instances) while staying
  competitive on SAT.

M5 decisions (live):

- **Miter pair:** ripple-carry (majority-gate carries) vs carry-lookahead
  (generate/propagate) — same function, different gate structure, so
  equivalence is a real question. A carry-select adder was the more
  "different" alternative; lookahead won on simplicity.
- **Negative control:** `--buggy` drops the propagate term at one carry bit.
  An UNSAT-only demo is unconvincing; the SAT counterexample (a=8 b=10 cin=1:
  ripple says 19, buggy lookahead says 51) shows the miter actually
  distinguishes circuits.
- **Sudoku as uniqueness proof:** solve, add the blocking clause of the found
  solution, re-solve with proof on. UNSAT = the solution is unique, and the
  DRAT file is the receipt for that claim. This is the project's thesis in
  miniature; a general Sudoku tool was deliberately not built.
- **Sudoku encoding:** cell exactly-one plus pairwise at-most-one per row,
  column, and box — no at-least-one clauses per unit. Pigeonhole makes the
  encoding complete anyway (nine cells, nine digits, no repeats forces every
  digit to appear), so the extended encoding's extra 243 clause groups were
  cut as redundant.
- **Puzzle source:** Wikipedia's standard example grid, trusted to be
  unique. The trust is checked at runtime, not assumed: a non-unique puzzle
  would surface as SAT on the blocked re-solve, so the demo cannot silently
  claim a false uniqueness.
- **`--dimacs` dump flags** on both demos, added because drat-trim needs the
  CNF alongside the proof — the internal formula had to become a file.
- **Evidence-page numbers recomputed, not copied.** The SATLIB tables on
  `docs/index.html` came from a throwaway Python aggregation over the
  committed CSVs, then cross-checked against HANDOFF and README (both
  matched: 25 and 414 timeouts / 6399). Three independent sources agreeing
  beats one source transcribed.
- **Demo outputs are scratch.** `miter*.cnf/.drat` and `sudoku.cnf/.drat`
  at the repo root were deleted and gitignored (`*.cnf`, `*.drat`) rather
  than committed — one command regenerates them, and the README says which.
- **Descoped:** incremental solving under assumptions (spec marked it
  optional-if-time); the Proof Foundry overnight fleets (workflow
  opt-in is per-session and nobody was watching this session).

## 4. The oracle problem — trusting a solver you just wrote

The crux of the whole build: a solver bug that flips a verdict is invisible
from inside. The project's answer was to never trust itself:

1. **Differential testing** against minisat on 10,000 fuzzed formulas per
   heuristic (phase-transition 3-SAT at ratio 4.26, off-ratio, 2/4-SAT,
   pigeonhole, graph coloring). Divergences fed a ddmin shrinker
   (`src/bin/shrink.rs`) that files minimal repros with minisat in the loop.
2. **Proof checking** by drat-trim on every UNSAT instance — 3849 (EVSIDS,
   seed 42) and 3870 (VMTF, seed 43) proofs, all `s VERIFIED`, including
   deletion lines after M4.
3. **Ground truth** on SATLIB: uf series are known SAT, uuf known UNSAT;
   every one of 6399 verdicts matched, and SAT models were checked against
   the formula.

The oracles live in WSL Ubuntu 24.04 (minisat via apt, drat-trim built from
source at `/usr/local/bin/drat-trim`), which produced its own friction — see
section 6.

## 5. Tools and features used

| Tool / feature | What it did this session (M5) |
|---|---|
| `HANDOFF.md` + `handoff-after-clear` skill | Cold-start resume; all environment facts held |
| advisor (2 calls) | Opening: locked M5 work order, added negative control + uniqueness framing. Closing: caught the missing M5 hash in HANDOFF.md |
| Bash tool (Git Bash) | cargo builds, WSL oracle runs, git commits via `git commit -F -` |
| WSL (`wsl -u root -e bash -c`) | drat-trim verification of miter and Sudoku proofs |
| `dev-journey` skill | This document |
| Write/Edit tools | `src/bin/miter.rs`, `src/bin/sudoku.rs`, `docs/index.html` |

Model: Claude Fable 5, ponytail (minimal-diff) mode active, replies in
ASD-STE100 style — both standing modes from the user's global config, which
shaped the code toward small single-file binaries with no new dependencies.
No subagents were spawned in the M5 session; the work was inline. (M2's
commit references the fuzz fleet design but the transcript that would show
subagent use in M1–M4 is gone.)

## 6. What went wrong, and the fixes

From the M1–M4 record (HANDOFF's "environment facts, verified this session"
— these are the survivors of real debugging, error text not preserved):

1. **WSL apt mirror hash-sum mismatches** blocked installing minisat. Fix:
   switch `/etc/apt/sources.list.d/ubuntu.sources` to https.
2. **CRLF breaks bash in WSL.** Gate scripts written on Windows had to be run
   through `tr -d '\r'` before bash would take them.
3. **Long `wsl -e bash -c "..."` one-liners fail** with "filename or
   extension is too long." Fix: put scripts in files.
4. **PowerShell mangles heredocs and quotes in `git commit -m`.** Fix: the
   Bash tool with `git commit -F - <<'MSG'`.
5. **SATLIB's dubois files are malformed** — header declares 800 clauses,
   only 598 are `0`-terminated. minisat reads them the same wrong way, so
   they were excluded rather than "fixed"; documented as a deviation.
6. **uuf200-860 ships 99 files, not 100** — a data quirk, recorded so the
   totals (6399, not 6400) don't look like a lost instance.

Live in M5:

7. **Exit code 20 read as failure.** The miter's minisat-convention UNSAT
   exit code stopped a `&&` command chain mid-run. Not a bug — the
   convention working as designed — but a near-miss: for a moment the output
   looked like a build failure. Rule learned: a solver that speaks minisat's
   exit codes makes `&&` chains lie.
8. **drat-trim needs the CNF, not just the proof.** Both demos initially
   wrote only `.drat`; the `--dimacs` flag was added so the internally built
   formula becomes a checkable file pair.
9. **HANDOFF.md briefly lied about M5.** It was rewritten to say
   "M5 (latest commit)" *before* that commit existed — the only milestone
   entry without a hash. The closing advisor review caught it; the hash
   `5a998ac` was patched in after the push. Rule: a resume file written
   before its commit lies about the one identifier that matters.
10. **GitHub Pages 404s at first.** The `gh api ... /pages` enable call
   succeeded immediately, but the live URL returned 404 three times over
   ~60 s before the first 200. The poll loop, not the API response, was the
   verification — a clean API call is not a deployed page.

## 7. Verification

Everything below is an observed effect, not a clean exit:

- **Miter, 8-bit and 16-bit:** `s UNSATISFIABLE`, then drat-trim
  `s VERIFIED` on both proof/CNF pairs (0.089 s and 0.075 s).
- **Miter negative control:** `--buggy` → `s SATISFIABLE`, counterexample
  decoded and cross-checked against true arithmetic (19 vs 51).
- **Sudoku:** solves Wikipedia's standard example puzzle, printed grid checked
  by eye; blocked re-solve → `s UNSATISFIABLE`; uniqueness receipt
  `s VERIFIED` by drat-trim (0.089 s).
- **Benchmark tables** on the evidence page recomputed from the committed
  CSVs, not copied from memory; totals (25 and 414 timeouts / 6399) match
  both HANDOFF and README independently.
- **Push** verified with `git ls-remote` against the commit hash.

Not verified: GitHub Pages rendering across browsers (curl for HTTP 200 on
the live URL is the check that ran); the M1–M4 gates were not re-run in this
session — the commit messages and CSVs are the record.

## 8. Where things stand

The five-milestone spec is complete. Public repo:
https://github.com/az9713/unsat-with-receipts — solver (`unsat_with_receipts`),
demos (`miter`, `sudoku`), fuzz harness (`fuzz`, `shrink`, `bench`), gate
scripts (`scripts/`), per-instance benchmark CSVs, evidence page
(`docs/index.html`, GitHub Pages).

Open options, not promises:

- **Incremental solving under assumptions** — the spec's optional extension;
  the solver's `with_config` API is the natural seam.
- **Proof Foundry wave-3 fleets** — the overnight adversarial-proof runner
  was never spawned (template at
  `Downloads\projects\fable_5_maxxing_3_me\.claude\workflows\adversarial-proofs.js`,
  fleets pinned to Opus/Sonnet). Requires a fresh per-session opt-in.
- **hole10** remains the one unsolved instance in the 60 s tier — pigeonhole
  is exponential for resolution, so this is expected, not a defect.

Knowledge captured: `HANDOFF.md` updated to mark M5 done; the WSL oracle
setup is in the auto-memory index (`wsl-sat-oracles.md`); everything else
load-bearing is in the repo itself.
