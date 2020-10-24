#!/bin/bash

set -euo pipefail

cross build --release --target armv7-unknown-linux-gnueabihf --bin mijia-homie

cd mijia-homie
cargo deb --target armv7-unknown-linux-gnueabihf --no-build
cargo deb
