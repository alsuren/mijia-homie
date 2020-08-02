set -euxo pipefail

# This relies on a version of cross that can read `context` and `dockerfile`
# from Cross.toml. You can install it with:
#
#     cargo install --git=https://github.com/alsuren/cross --branch=docker-build-context
#
time cross build --target armv7-unknown-linux-gnueabihf --release
time rsync target/armv7-unknown-linux-gnueabihf/release/read-all-devices pi@raspberrypi.local:read-all-devices
time rsync target/armv7-unknown-linux-gnueabihf/release/publish-mqtt pi@raspberrypi.local:publish-mqtt
time ssh pi@raspberrypi.local ./publish-mqtt
