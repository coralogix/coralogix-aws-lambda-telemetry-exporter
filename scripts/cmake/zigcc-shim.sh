#!/bin/bash
set -euo pipefail

zig_target="${1:?zig cargo-lambda target required (e.g. aarch64-linux-gnu.2.26)}"
shift

args=("$@")
wants_assembly=false
for arg in "${args[@]}"; do
  if [[ "$arg" == "-S" ]]; then
    wants_assembly=true
    break
  fi
done

if [[ "$wants_assembly" == true ]]; then
  # AWS-LC FIPS compiles bcm.c with -S, archives that textual assembly, then feeds it to delocate.
  # cargo-lambda's Zig wrapper skips that assembly output when CMake also passes object/dependency flags.
  # So here we remove those flags. https://github.com/aws/aws-lc/issues/3261
  filtered=()
  skip_next=false
  for arg in "${args[@]}"; do
    if [[ "$skip_next" == true ]]; then
      skip_next=false
      continue
    fi
    [[ "$arg" == "-c" ]] && continue
    [[ "$arg" == "-MD" ]] && continue
    if [[ "$arg" == "-MT" || "$arg" == "-MF" ]]; then
      skip_next=true
      continue
    fi
    filtered+=("$arg")
  done
  args=("${filtered[@]}")
fi

exec cargo-lambda zig cc -- -target "$zig_target" "${args[@]}"
