#!/usr/bin/env bash
# shellcheck disable=SC2002

set -euxo pipefail
shellcheck "$0"

cd "$(dirname "$0")"

## Set STATE_FILE=/path/to/some/file to change where your state file lives (in case you need to re-run the script)
STATE_FILE="./install-rpi.state"
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

## Set FINAL_HOSTNAME=yourfavouritepi.local to change the hostname that the raspberrypi will take
FINAL_HOSTNAME=${FINAL_HOSTNAME:-cottagepi.local}
FINAL_SSH=${FINAL_SSH:-pi@$FINAL_HOSTNAME}

## Set BACKUP_SSH=user@host.local to decide which machine to backup configs from.
BACKUP_HOSTNAME=${BACKUP_HOSTNAME:-${FINAL_HOSTNAME}}
BACKUP_SSH=${BACKUP_SSH:-pi@${BACKUP_HOSTNAME}}

## Set BOOTSTRAP_SSH=user@host.local to specify where you expect the raspberrypi to appear on first boot
BOOTSTRAP_HOSTNAME=${BOOTSTRAP_HOSTNAME:-raspberrypi.local}
BOOTSTRAP_SSH=${BOOTSTRAP_SSH:-pi@${BOOTSTRAP_HOSTNAME}}

## Set SSH_IMPORT_IDS='gh:alsuren gh:qwandor' to add ssh keys to your raspberry pi
SSH_IMPORT_IDS=${SSH_IMPORT_IDS:-'gh:alsuren gh:qwandor'}

## Set WIFI_COUNTRY="us" to specify your wifi country.
WIFI_COUNTRY=${WIFI_COUNTRY:-"gb"}
## Set WIFI_SSID="yourwifissid" to specify your wifi ssid.
WIFI_SSID=${WIFI_SSID:?Please set WIFI_SSID}
## Set WIFI_PSK="yourwifipassword" to specify your wifi password.
WIFI_PSK=${WIFI_PSK:?Please set WIFI_PSK}

## Set SDCARD=/Volumes/mountpoint/ to specify sdcard location.
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
    STEP="${STEP:-1}"
fi

if [[ "$STEP" == 1 ]]; then
    echo "backing up from ${BACKUP_SSH}"
    ssh "${BACKUP_SSH}" sudo tar -c -f - /etc/mijia-homie /etc/telegraf/telegraf.conf > "${BACKUP_SSH}.etc.tar"
    tar -t -f "${BACKUP_SSH}.etc.tar"

    inc_step
fi

if [[ "$STEP" == 2 ]]; then
    # shellcheck disable=SC2016
    echo '
    NOTICE: This script does not handle flashing of SD cards.
    You probably want to download an image from
        https://downloads.raspberrypi.org/raspios_arm64/images/
    and then do something like:
        sudo dd if=~/Downloads/2021-10-30-raspios-bullseye-arm64.img of=/dev/rdisk2 bs=$((1024 * 1024 * 4))
    '
    until [[ -d "$SDCARD" ]]; do
        echo "NOTICE: Waiting for $SDCARD to be mounted. Press enter to try again."
        # automatic retry in 10 seconds if the user doesn't do anything
        read -rt 10 || true
    done
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
        IP=$(ssh-copy-id -oStrictHostKeyChecking=no "${FINAL_SSH}" ip address show dev wlan0 | grep ' inet ' | sed -e 's:/.*::' -e 's/^.* //')
        ssh-keygen -R "$IP"
        ssh-keyscan -H "${FINAL_HOSTNAME}" >> ~/.ssh/known_hosts
        ssh-keyscan -H "${IP}" >> ~/.ssh/known_hosts
    fi
    inc_step
fi

if [[ "$STEP" == 5 ]]; then
    ssh "${FINAL_SSH}" hostname 
    ssh "${FINAL_SSH}" 'curl https://sh.rustup.rs -sSf | sh -s -- -y'
    inc_step
fi

if [[ "$STEP" == 6 ]]; then
    # shellcheck disable=SC2086
    ssh "${FINAL_SSH}" ssh-import-id $SSH_IMPORT_IDS
    inc_step
fi

if [[ "$STEP" == 7 ]]; then
    cat "${BACKUP_SSH}.etc.tar" | \
        ssh "${FINAL_SSH}" sudo tar -x -v -f - -C /
    inc_step
fi

if [[ "$STEP" == 8 ]]; then
    curl -L https://homiers.jfrog.io/artifactory/api/security/keypair/public/repositories/homie-rs | ssh "${FINAL_SSH}" sudo apt-key add -
    echo "deb https://homiers.jfrog.io/artifactory/homie-rs stable main" | ssh "${FINAL_SSH}" sudo tee /etc/apt/sources.list.d/homie-rs.list
    ssh "${FINAL_SSH}" sudo apt update
    ssh "${FINAL_SSH}" sudo apt install mijia-homie

    inc_step
fi

if [[ "$STEP" == 9 ]]; then

    echo unattended-upgrades unattended-upgrades/enable_auto_updates boolean true | ssh "${FINAL_SSH}" sudo debconf-set-selections
    ssh "${FINAL_SSH}" sudo apt install unattended-upgrades apt-listchanges

    inc_step
fi

if [[ "$STEP" == 10 ]]; then

    ssh "${FINAL_SSH}" sudo passwd --delete pi

    inc_step
fi

if [[ "$STEP" == 11 ]]; then

    ssh "${FINAL_SSH}" sudo apt install -y vim mc shellcheck

    inc_step
fi

if [[ "$STEP" == 12 ]]; then
    VERSION_CODENAME=$(ssh "${FINAL_SSH}" grep VERSION_CODENAME /etc/os-release | sed s/VERSION_CODENAME=//)

    curl -s https://repos.influxdata.com/influxdb.key | ssh "${FINAL_SSH}" sudo apt-key add -
    echo "deb https://repos.influxdata.com/debian ${VERSION_CODENAME} stable" | ssh "${FINAL_SSH}" sudo tee /etc/apt/sources.list.d/influxdb.list

    ssh "${FINAL_SSH}" sudo apt update
    ssh "${FINAL_SSH}" sudo apt install telegraf
    ssh "${FINAL_SSH}" sudo systemctl start telegraf

    inc_step
fi
