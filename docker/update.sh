#!/usr/bin/env bash

set -euo pipefail

docker build ./docker -f docker/Dockerfile.debian-buster-aarch64 \
    -t ghcr.io/qwandor/cross-dbus-debian-buster-aarch64:latest
docker build ./docker -f docker/Dockerfile.debian-buster-armv7 \
    -t ghcr.io/qwandor/cross-dbus-debian-buster-armv7:latest
docker build ./docker -f docker/Dockerfile.debian-buster-x86_64 \
    -t ghcr.io/qwandor/cross-dbus-debian-buster-x86_64:latest

docker push ghcr.io/qwandor/cross-dbus-debian-buster-aarch64:latest
docker push ghcr.io/qwandor/cross-dbus-debian-buster-armv7:latest
docker push ghcr.io/qwandor/cross-dbus-debian-buster-x86_64:latest
