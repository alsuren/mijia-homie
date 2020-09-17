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
USE_SYSTEMD=${USE_SYSTEMD:-1}

if [ $PROFILE = release ]; then
    PROFILE_FLAG=--release
elif [ $PROFILE = debug ]; then
    PROFILE_FLAG=''
else
    echo "Invalid profile '$PROFILE'"
    exit 1
fi

time cross build $PROFILE_FLAG --target $TARGET --bin publish-mqtt

time rsync --progress target/$TARGET/$PROFILE/publish-mqtt $TARGET_SSH:publish-mqtt

SERVICE_FILE="
[Unit]
Description=publish-mqtt - send temperature readings to mqtt
After=network.target

[Service]
Type=simple
User=pi
WorkingDirectory=/home/pi
Environment=RUST_BACKTRACE=1
ExecStart=/home/pi/publish-mqtt
Restart=always

[Install]
WantedBy=multi-user.target
"

if [ $RUN = 1 ]; then
    if [ $USE_SYSTEMD = 1 ]; then
        echo $SERVICE_FILE | ssh $TARGET_SSH sudo tee /etc/systemd/system/publish-mqtt.service > /dev/null
        ssh $TARGET_SSH sudo systemctl restart publish-mqtt.service
        ssh $TARGET_SSH sudo journalctl -u publish-mqtt.service --output=cat --follow
    else
        ssh $TARGET_SSH ./publish-mqtt
    fi
fi
