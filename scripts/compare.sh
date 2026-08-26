#!/bin/bash
# Run minisat over the fuzz corpus and diff verdicts against ours.txt.
# Usage (from repo root, inside WSL): bash scripts/compare.sh fuzz-corpus
cd "$1" || exit 1
: > theirs.txt
for f in *.cnf; do
  i="${f%.cnf}"
  minisat -verb=0 "$f" >/dev/null 2>&1
  case $? in
    10) echo "$i SAT" >> theirs.txt ;;
    20) echo "$i UNSAT" >> theirs.txt ;;
    *)  echo "$i ERR" >> theirs.txt ;;
  esac
done
sort -n ours.txt > o.s
sort -n theirs.txt > t.s
diff o.s t.s > divergences.txt
echo "diff_exit=$?"
echo "divergence_lines=$(wc -l < divergences.txt)"
echo "minisat_sat=$(grep -c ' SAT' t.s)"
echo "minisat_unsat=$(grep -c UNSAT t.s)"
