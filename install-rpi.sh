#!/usr/bin/env bash

set -euxo pipefail
shellcheck -e SC2002 "$0"

## Set STATE_FILE=/path/to/some/file to change where your state file lives (in case you need to re-run the script)
STATE_FILE="$TMPDIR/install-rpi.state"
function get_state() {
    KEY="$1"
    declare -g "$KEY"

    touch "$STATE_FILE"
    eval "$(cat "$STATE_FILE" | grep "^$KEY=")"
}

function set_state() {
    KEY="$1"
    VALUE="$2"
    echo "$KEY=$VALUE" >> "$STATE_FILE"
    tac "$STATE_FILE" | sort -t "=" -k 1,1 -s --unique | tac > "$STATE_FILE".dedup
    mv "$STATE_FILE".dedup "$STATE_FILE"
    get_state "$KEY"
}

function inc_step() {
    set_state STEP $(("$STEP" + 1))
}

## Set BACKUP_SSH=user@host.local to decide which machine to backup configs from.
BACKUP_SSH=${BACKUP_SSH:-pi@cottagepi.local}
## Set BOOTSTRAP_SSH=user@host.local to specify where you expect the raspberrypi to appear on first boot
BOOTSTRAP_SSH=${BOOTSTRAP_SSH:-pi@raspberrypi.local}
## Set FINAL_HOSTNAME=yourfavouritepi.local to change the hostname that the raspberrypo will take
FINAL_HOSTNAME=${FINAL_HOSTNAME:-${BACKUP_SSH#*@}}
FINAL_SSH=${FINAL_SSH:-pi@$FINAL_HOSTNAME}

## Set SSH_IMPORT_IDS='gh:alsuren gh:qwandor' to add ssh keys to your raspberry pi
SSH_IMPORT_IDS=${SSH_IMPORT_IDS:?"Set SSH_IMPORT_IDS='gh:alsuren gh:qwandor' to add ssh keys to your raspberry pi"}

## Set WIFI_COUNTRY="us" to specify your wifi country.
WIFI_COUNTRY=${WIFI_COUNTRY:-"gb"}
## Set WIFI_SSID="yourwifissid" to specify your wifi ssid.
WIFI_SSID=${WIFI_SSID:?Please set WIFI_SSID}
## Set WIFI_PSK="yourwifipassword" to specify your wifi password.
WIFI_PSK=${WIFI_PSK:?Please set WIFI_PSK}

## Set SDCARD=/Volumes/monutpoint/ to specify sdcard location.
SDCARD=${SDCARD:-/Volumes/boot/}

if [ $# != 0 ]; then
    echo "ERROR: $0 should be configured via the following environment variables:"
    echo
    grep '^## ' "$0" | sed 's/^## /  /'
    echo
    exit 1
fi

STEP="${STEP:-}"
if [[ "$STEP" == "" ]]; then
    get_state STEP
fi

if [[ "$STEP" == 1 ]]; then
    echo "backing up from ${BACKUP_SSH}"
    ssh "${BACKUP_SSH}" sudo tar -c -f - /etc/mijia-homie > "${BACKUP_SSH}.etc.mijia-homie.tar"
    tar -t -f "${BACKUP_SSH}.etc.mijia-homie.tar"

    inc_step
fi

if [[ "$STEP" == 2 ]]; then
    echo "setting up sdcard at $SDCARD for unattended installs"
    cat > "$SDCARD/wpa_supplicant.conf" << EOF
country=$WIFI_COUNTRY
update_config=1
ctrl_interface=/var/run/wpa_supplicant
network={
 scan_ssid=1
 ssid="$WIFI_SSID"
 psk="$WIFI_PSK"
}
EOF
    cat "$SDCARD/wpa_supplicant.conf"
    touch "$SDCARD/ssh"
    diskutil eject "$SDCARD"

    inc_step
fi

if [[ "$STEP" == 3 ]]; then
    ssh-keygen -R "${BOOTSTRAP_SSH#*@}"
    echo "please plug your sdcard into your raspberry pi and restart"
    echo "setting up ${BOOTSTRAP_SSH}. If asked for a password, type 'raspberry'"
    # FIXME: use ssh-import-id here instead
    ssh-copy-id -oStrictHostKeyChecking=no "${BOOTSTRAP_SSH}"

    inc_step
fi

if [[ "$STEP" == 4 ]]; then
    if ! ssh -oConnectTimeout=5 -oStrictHostKeyChecking=no "${FINAL_SSH}" true ; then
        echo "changing your raspberry pi's hostname and restarting"
        ssh-keygen -R "$FINAL_HOSTNAME"
        echo "$FINAL_HOSTNAME" | ssh "${BOOTSTRAP_SSH}" sudo tee /etc/hostname
        if ! ssh "${BOOTSTRAP_SSH}" grep "$FINAL_HOSTNAME" /etc/hosts; then
            echo "127.0.1.1        ${FINAL_HOSTNAME%%.*}" | ssh "${BOOTSTRAP_SSH}" sudo tee -a /etc/hosts

            ssh "${BOOTSTRAP_SSH}" sudo reboot

        fi
        ssh -oStrictHostKeyChecking=no "${FINAL_SSH}" hostname
    fi
    inc_step
fi

if [[ "$STEP" == 5 ]]; then
    ssh -oStrictHostKeyChecking=no "${FINAL_SSH}" hostname 
    ssh "${FINAL_SSH}" 'curl https://sh.rustup.rs -sSf | sh -s -- -y'
    inc_step
fi

if [[ "$STEP" == 6 ]]; then
    # shellcheck disable=SC2086
    ssh "${FINAL_SSH}" ssh-import-id $SSH_IMPORT_IDS
    inc_step
fi

if [[ "$STEP" == 7 ]]; then
    cat "${BACKUP_SSH}.etc.mijia-homie.tar" | \
        ssh "${FINAL_SSH}" sudo tar -x -v -f - -C /
    inc_step
fi

if [[ "$STEP" == 8 ]]; then
    ARCH="$(ssh "${FINAL_SSH}" uname --machine)"
    VERSION="$(git tag | sed -n s/mijia-homie-//p | tail -n1)"
    if [[ "$ARCH" != aarch64 ]] ; then
        echo "TODO: think about the other rpi architectures"
        exit 1
    fi
    
    ssh "${FINAL_SSH}" curl -sSLf "https://github.com/alsuren/mijia-homie/releases/download/mijia-homie-${VERSION}/mijia-homie_${VERSION}_arm64.deb" -o "mijia-homie_${VERSION}_arm64.deb" 
    ssh "${FINAL_SSH}" sudo dpkg -i "mijia-homie_${VERSION}_arm64.deb"
fi