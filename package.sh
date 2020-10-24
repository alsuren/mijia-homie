#!/bin/bash

# Build a Debian package for the given target, or the default host target if none is set.

set -euo pipefail

TARGET=${TARGET:-}

if [ -z "$TARGET" ]; then
  cd mijia-homie
  cargo deb
else
  cross build --release --target "$TARGET" --bin mijia-homie
  cd mijia-homie
  cargo deb --target "$TARGET" --no-build
fi
