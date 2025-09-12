#!/usr/bin/env bash

set -euo pipefail

VERSION=0.2.1-2

docker build ./docker -f docker/Dockerfile.debian-trixie-aarch64 \
    -t ghcr.io/qwandor/cross-dbus-debian-trixie-aarch64:$VERSION
docker build ./docker -f docker/Dockerfile.debian-trixie-armv7 \
    -t ghcr.io/qwandor/cross-dbus-debian-trixie-armv7:$VERSION
docker build ./docker -f docker/Dockerfile.debian-trixie-armv6 \
    -t ghcr.io/qwandor/cross-dbus-debian-trixie-armv6:$VERSION
docker build ./docker -f docker/Dockerfile.debian-trixie-x86_64 \
    -t ghcr.io/qwandor/cross-dbus-debian-trixie-x86_64:$VERSION

docker push ghcr.io/qwandor/cross-dbus-debian-trixie-aarch64:$VERSION
docker push ghcr.io/qwandor/cross-dbus-debian-trixie-armv7:$VERSION
docker push ghcr.io/qwandor/cross-dbus-debian-trixie-armv6:$VERSION
docker push ghcr.io/qwandor/cross-dbus-debian-trixie-x86_64:$VERSION
