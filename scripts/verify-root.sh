#!/bin/sh
# Verify that running with the needed privilege gives full PSS coverage.
#
#   sudo scripts/verify-root.sh
#   METOP_BIN=/path/to/metop scripts/verify-root.sh   # non-default binary
#
# Exit codes: 0 OK, 1 assertions failed, 2 setup problem.
# See docs/VERIFY-ROOT.md for what each number means.
set -eu

bin="${METOP_BIN:-./target/release/metop}"
if [ ! -x "$bin" ]; then
    echo "not executable: $bin (run: cargo build --release)" >&2
    exit 2
fi

tmp=$(mktemp "${TMPDIR:-/tmp}/metop-verify.XXXXXX")
trap 'rm -f "$tmp"' EXIT INT TERM

# Only the fields the checks need: keeps the snapshot small on big machines.
"$bin" --json \
    --fields sampling,procs.pid,procs.comm,procs.rss_kb,procs.pss.pss_kb,procs.pss.rss_kb \
    > "$tmp"

python3 - "$tmp" <<'PY'
import json
import sys

d = json.load(open(sys.argv[1]))
s = d["sampling"]
procs = d["procs"]

with_pss = [p for p in procs if p.get("pss")]
bad = [p for p in with_pss if p["pss"]["pss_kb"] > p["pss"]["rss_kb"] + 1]
total = sum(p["pss"]["pss_kb"] for p in with_pss) / 1024 / 1024

print(f"procs={s['procs']} covered={s['covered']} denied={s['denied']} no_mm={s['no_mm']}")
print(f"pss total={total:.2f} GiB  pss<=rss violations={len(bad)}")

ok = True
if s["denied"] != 0:
    need = s["denied"]
    print(f"FAIL: {need} processes still return EACCES; see docs/VERIFY-ROOT.md section 4")
    ok = False
if bad:
    print(f"FAIL: {len(bad)} rows report PSS > RSS")
    ok = False
accounted = s["covered"] + s["no_mm"] + s["denied"]
if accounted < s["procs"] - 5:
    print(
        f"FAIL: covered {s['covered']} + no_mm {s['no_mm']} + denied {s['denied']}"
        f" = {accounted} does not account for {s['procs']} procs"
    )
    ok = False
if not with_pss:
    print("FAIL: not a single process had a readable PSS")
    ok = False

print("OK" if ok else "FAILED")
sys.exit(0 if ok else 1)
PY
