#!/bin/bash
# Download the SATLIB uniform random 3-SAT series into benchmarks/satlib/.
set -e
BASE=https://www.cs.ubc.ca/~hoos/SATLIB/Benchmarks/SAT/RND3SAT
mkdir -p benchmarks/satlib
cd benchmarks/satlib
for s in uf20-91 uf50-218 uf75-325 uf100-430 uf125-538 uf150-645 \
         uf175-753 uf200-860 uf225-960 uf250-1065 \
         uuf50-218 uuf75-325 uuf100-430 uuf125-538 uuf150-645 \
         uuf175-753 uuf200-860 uuf225-960 uuf250-1065; do
  if [ ! -d "$s" ]; then
    echo "fetching $s"
    curl -fsSL "$BASE/$s.tar.gz" -o "$s.tar.gz" || { echo "MISS $s"; continue; }
    mkdir -p "$s"
    tar xzf "$s.tar.gz" -C "$s"
    rm "$s.tar.gz"
  fi
done
find . -name '*.cnf' | wc -l
