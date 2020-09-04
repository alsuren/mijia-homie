set -euxo pipefail

# This relies on a version of cross that can read `context` and `dockerfile`
# from Cross.toml. You can install it with:
#
#     cargo install --git=https://github.com/alsuren/cross --branch=docker-build-context
#
TARGET=${TARGET:-armv7-unknown-linux-gnueabihf}
TARGET_SSH=${TARGET_SSH:-pi@raspberrypi.local}
PROFILE=${PROFILE:-debug}
RUN=${RUN:-1}
SUFFIX=${SUFFIX:-}
BIN=${BIN:-publish-mqtt}

if [ $PROFILE = release ]
then
    time cross build --target $TARGET --release
elif [ $PROFILE = debug ]
then
    time cross build --target $TARGET --bin $BIN
else
    echo "Invalid profile '$PROFILE'"
    exit 1
fi

time rsync --progress target/$TARGET/$PROFILE/$BIN $TARGET_SSH:$BIN$SUFFIX

if [ $RUN -eq 1 ]
then
    ssh $TARGET_SSH ./$BIN$SUFFIX
fi
