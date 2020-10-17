#!/bin/bash

set -euo pipefail

TARGET=${TARGET:-armv7-unknown-linux-gnueabihf}
TARGET_SSH=${TARGET_SSH:-pi@raspberrypi.local}
PROFILE=${PROFILE:-debug}
RUN=${RUN:-1}
USE_SYSTEMD=${USE_SYSTEMD:-1}
EXAMPLE=${EXAMPLE:-}

if [ $# != 0 ]; then
    echo "ERROR: $0 should be configured via the following environment variables:"
    echo
    cat $0 |  grep --only-matching '^[A-Z_]\+'
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
