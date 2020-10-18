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
## Set EXAMPLE=list-sensors to run the list-sensors example rather than publish-mqtt.
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

if [ $PROFILE = release ]; then
    PROFILE_FLAG=--release
elif [ $PROFILE = debug ]; then
    PROFILE_FLAG=''
else
    echo "Invalid profile '$PROFILE'"
    exit 1
fi

cargo install cross

if [ "${EXAMPLE}" != "" ]; then
    time cross build $PROFILE_FLAG --target $TARGET --example $EXAMPLE
    time rsync --progress target/$TARGET/$PROFILE/examples/$EXAMPLE $TARGET_SSH:$EXAMPLE
else
    time cross build $PROFILE_FLAG --target $TARGET --bin publish-mqtt
    time rsync --progress target/$TARGET/$PROFILE/publish-mqtt $TARGET_SSH:publish-mqtt
fi

if [ $RUN = 1 ]; then
    if [  $EXAMPLE != "" ]; then
        ssh $TARGET_SSH ./$EXAMPLE
    elif [ $USE_SYSTEMD = 1 ]; then
        scp publish-mqtt.service $TARGET_SSH:publish-mqtt.service
        ssh $TARGET_SSH sudo mv publish-mqtt.service /etc/systemd/system/publish-mqtt.service
        ssh $TARGET_SSH sudo systemctl daemon-reload
        ssh $TARGET_SSH sudo systemctl restart publish-mqtt.service
        ssh $TARGET_SSH sudo journalctl -u publish-mqtt.service --output=cat --follow
    else
        ssh $TARGET_SSH ./publish-mqtt
    fi
fi
