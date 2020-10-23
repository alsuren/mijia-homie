#!/bin/bash

set -euo pipefail

cross build --release --target armv7-unknown-linux-gnueabihf --bin mijia-homie
cargo deb -p mijia-homie --target armv7-unknown-linux-gnueabihf --no-build

cargo deb -p mijia-homie
