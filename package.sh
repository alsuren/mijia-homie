#!/bin/bash

# Build a Debian package for the given target, or the default host target if none is set.

set -euo pipefail

CRATE=${CRATE:-"mijia-homie"}
TARGET=${TARGET:-}

if [ -z "$TARGET" ]; then
  cd "$CRATE"
  cargo deb
else
  cross build --release --target "$TARGET" --bin "$CRATE"
  cd "$CRATE"
  cargo deb --target "$TARGET" --no-build
fi
