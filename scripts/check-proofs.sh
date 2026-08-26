#!/bin/bash
# Verify every DRAT proof in the corpus with drat-trim.
# Usage (inside WSL, from repo root): bash scripts/check-proofs.sh fuzz-corpus
DRAT=${DRAT:-/root/drat-trim/drat-trim}
cd "$1" || exit 1
total=0; ok=0
: > proof-failures.txt
for p in *.drat; do
  [ -e "$p" ] || { echo "no proofs found"; exit 1; }
  i="${p%.drat}"
  total=$((total+1))
  if "$DRAT" "$i.cnf" "$p" 2>/dev/null | grep -q "s VERIFIED"; then
    ok=$((ok+1))
  else
    echo "$i" >> proof-failures.txt
  fi
done
echo "verified=$ok/$total"
echo "failures=$(wc -l < proof-failures.txt)"
