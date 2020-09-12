set -euxo pipefail

# This relies on a version of cross that can read `context` and `dockerfile`
# from Cross.toml. You can install it with:
#
#     cargo install --git=https://github.com/alsuren/cross --branch=docker-build-context
#
TARGET=${TARGET:-armv7-unknown-linux-gnueabihf}
TARGET_SSH=${TARGET_SSH:-pi@raspberrypi.local}
PROFILE=${PROFILE:-debug}
RUN=${RUN:-0}
DEPLOY=${DEPLOY:-1}
SUFFIX=${SUFFIX:-}
BIN=${BIN:-publish-mqtt}

if [ $PROFILE = release ]; then
    PROFILE_FLAG=--release
elif [ $PROFILE = debug ]; then
    PROFILE_FLAG=''
else
    echo "Invalid profile '$PROFILE'"
    exit 1
fi

time cross build $PROFILE_FLAG --target $TARGET --bin $BIN

time rsync --progress target/$TARGET/$PROFILE/$BIN $TARGET_SSH:$BIN$SUFFIX

if [ $DEPLOY = 1 ]; then
    if [ $SUFFIX = '' ]; then
        echo "can't deploy with suffix"
        exit 1
    fi
    ssh $TARGET_SSH sudo systemctl restart $BIN.service
    ssh pi@raspberrypi.local sudo journalctl -u publish-mqtt.service --output=cat --follow
elif [ $RUN = 1 ]; then
    ssh $TARGET_SSH sudo systemctl stop $BIN.service || echo "Oh. Nevermind."
    ssh $TARGET_SSH killall $BIN $BIN$SUFFIX || echo "Oh. Nevermind."
    ssh $TARGET_SSH ./$BIN$SUFFIX
fi
