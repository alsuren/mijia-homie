#!/bin/bash

set -euo pipefail

## Set TARGET_SSH=user@host.local to decide which machine to run on.
TARGET_SSH=${TARGET_SSH:-pi@raspberrypi.local}
## Set PROFILE=release for a release build.
PROFILE=${PROFILE:-debug}
## Set RUN=0 to push a new binary without running it.
RUN=${RUN:-1}
## Set USE_SYSTEMD=0 to run without process supervision (EXAMPLEs never use process supervision).
USE_SYSTEMD=${USE_SYSTEMD:-1}
## Set EXAMPLE=list-sensors to run the list-sensors example rather than mijia-homie.
EXAMPLE=${EXAMPLE:-}

# Target architecture for Raspbian on a Raspberry Pi.
# Changing this requires changes to Cross.toml. Send a patch if you want this
# to be made configurable again.
TARGET=armv7-unknown-linux-gnueabihf

if [ $# != 0 ]; then
    echo "ERROR: $0 should be configured via the following environment variables:"
    echo
    grep '^## ' "$0" | sed 's/^## /  /'
    echo
    exit 1
fi

if [ "$PROFILE" = release ]; then
    PROFILE_FLAG=--release
elif [ "$PROFILE" = debug ]; then
    PROFILE_FLAG=''
else
    echo "Invalid profile '$PROFILE'"
    exit 1
fi

cargo install cross

if [ "${EXAMPLE}" != "" ]; then
    time cross build "$PROFILE_FLAG" --target $TARGET --example "$EXAMPLE"
    time rsync --progress "target/$TARGET/$PROFILE/examples/$EXAMPLE" "$TARGET_SSH:$EXAMPLE"
else
    time cross build "$PROFILE_FLAG" --target $TARGET --bin mijia-homie
    time rsync --progress "target/$TARGET/$PROFILE/mijia-homie" "$TARGET_SSH:mijia-homie"
fi

if [ "$RUN" = 1 ]; then
    if [ "$EXAMPLE" != "" ]; then
        # shellcheck disable=SC2029
        ssh "$TARGET_SSH" "./$EXAMPLE"
    elif [ "$USE_SYSTEMD" = 1 ]; then
        scp mijia-homie.service "$TARGET_SSH:mijia-homie.service"
        ssh "$TARGET_SSH" sudo mv mijia-homie.service /etc/systemd/system/mijia-homie.service
        ssh "$TARGET_SSH" sudo systemctl daemon-reload
        ssh "$TARGET_SSH" sudo systemctl restart mijia-homie.service
        ssh "$TARGET_SSH" sudo journalctl -u mijia-homie.service --output=cat --follow
    else
        ssh "$TARGET_SSH" ./mijia-homie
    fi
fi
