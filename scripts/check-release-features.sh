#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Checks the output of `cargo build -p kropa --release --locked --message-format=json` (lot 2.1d, review
# 2.1a m1): `kropa_ipc` and `kropa_capture` must be compiled WITHOUT their `test-support` feature, and no
# fake application (`kropa-ipc-fake`) may have been produced. Feature unification under
# `--workspace --all-targets` can switch the feature on in CI builds; a release must not carry it.
#
# Usage: scripts/check-release-features.sh BUILD.json [TARGET_DIR]
set -euo pipefail

json=${1:?usage: check-release-features.sh BUILD.json [TARGET_DIR]}
target_dir=${2:-}
status=0

for crate in kropa_ipc kropa_capture; do
  # The library must have been built, or the check below would prove nothing.
  built=$(jq -s --arg c "$crate" \
    '[.[] | select(.reason == "compiler-artifact" and .target.name == $c and (.target.kind | index("lib")))] | length' \
    "$json")
  if [ "$built" -lt 1 ]; then
    echo "error: $crate does not appear in $json; the check is blind" >&2
    status=1
    continue
  fi
  with=$(jq -s --arg c "$crate" \
    '[.[] | select(.reason == "compiler-artifact" and .target.name == $c and (.features | index("test-support")))] | length' \
    "$json")
  if [ "$with" -ne 0 ]; then
    echo "error: $crate was compiled with the test-support feature" >&2
    status=1
  else
    echo "ok: $crate compiled without test-support"
  fi
done

fake=$(jq -s \
  '[.[] | select(.reason == "compiler-artifact" and (.target.name == "kropa-ipc-fake" or ((.executable // "") | test("kropa-ipc-fake"))))] | length' \
  "$json")
if [ "$fake" -ne 0 ]; then
  echo "error: the fake application kropa-ipc-fake was built" >&2
  status=1
else
  echo "ok: no kropa-ipc-fake in the build output"
fi

if [ -n "$target_dir" ] && [ -e "$target_dir/kropa-ipc-fake" ]; then
  echo "error: $target_dir/kropa-ipc-fake exists" >&2
  status=1
fi

exit "$status"
