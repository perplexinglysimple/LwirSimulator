#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build --quiet --bins

SIM="target/debug/vliw_simulator"
VERIFY="target/debug/vliw_verify"
OUT_DIR="target/example-checks"
mkdir -p "$OUT_DIR/traces" "$OUT_DIR/verifier"

verify_clean() {
  local fixture="$1"
  local output="$OUT_DIR/verifier/$(basename "$fixture").txt"

  echo "verify clean: $fixture"
  "$VERIFY" "$fixture" > "$output"
  grep -q "Result  : CLEAN" "$output"
}

simulate_trace() {
  local fixture="$1"
  local trace="$OUT_DIR/traces/$(basename "$fixture").trace"

  echo "simulate trace: $fixture"
  "$SIM" --trace "$fixture" > "$trace"
  grep -q "^trace v1 width=" "$trace"
  grep -q "^final pc=.*halted=true$" "$trace"
}

expected_rule_line() {
  sed -nE 's/^# expected-rules?:[[:space:]]*//p' "$1" | head -n1
}

expected_status_line() {
  sed -nE 's/^# expected-status:[[:space:]]*//p' "$1" | head -n1
}

verify_expected_failure() {
  local fixture="$1"
  local output="$OUT_DIR/verifier/$(basename "$fixture").txt"
  local expected_status
  local expected_rules
  local status

  expected_status="$(expected_status_line "$fixture")"
  expected_rules="$(expected_rule_line "$fixture")"

  echo "verify expected failure: $fixture"
  set +e
  "$VERIFY" "$fixture" > "$output" 2>&1
  status=$?
  set -e

  if [[ "$expected_status" == "parse-error" ]]; then
    test "$status" -eq 2
    return
  fi

  if [[ -z "$expected_rules" ]]; then
    echo "$fixture: missing # expected-rule(s): metadata" >&2
    return 1
  fi

  test "$status" -eq 1
  local rules="${expected_rules//,/ }"
  local rule
  for rule in $rules; do
    grep -q "$rule" "$output"
  done
}

has_multi_cpu_topology() {
  grep -Eq 'cpus[[:space:]]+[2-9][0-9]*' "$1"
}

for fixture in examples/*.vliw; do
  case "$fixture" in
    examples/illegal_*.vliw)
      verify_expected_failure "$fixture"
      ;;
    *)
      verify_clean "$fixture"
      simulate_trace "$fixture"
      ;;
  esac
done

for fixture in examples/fixtures/legal/*.vliw; do
  if has_multi_cpu_topology "$fixture"; then
    echo "skip single-program CLI checks for multi-CPU fixture: $fixture"
  else
    verify_clean "$fixture"
    simulate_trace "$fixture"
  fi
done

cargo test --quiet coherence_fixture_pair

for fixture in examples/fixtures/illegal/*.vliw; do
  verify_expected_failure "$fixture"
done

echo "example checks passed"
