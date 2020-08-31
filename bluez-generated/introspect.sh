#!/usr/bin/env bash
# Bash >= 4 does not come with osx because of its GPLv3 license.
# Install it via homebrew to get associative array support.

set -euxo pipefail

cd "$(dirname "$0")"

if [ ${INTROSPECT:-1} = 1 ]; then
    ssh pi@raspberrypi.local \
        gdbus introspect --system --dest org.bluez --object-path / --recurse \
        | grep -E '^ *(node|interface) .* {$' \
        | (
            declare -A interface_to_path

            while read keyword value _bracket; do
                if [ $keyword = 'node' ]; then
                    current_path=$value
                elif [ $keyword = 'interface' ]; then
                    interface_to_path[${value}]=$current_path
                    echo ${interface_to_path[${value}]}
                else
                    echo "unexpected line $keyword $value $_bracket"
                    exit 1
                fi
            done

            for interface in ${!interface_to_path[@]}; do
                echo $interface -- ${interface_to_path[${interface}]}
                ssh pi@raspberrypi.local \
                    gdbus introspect \
                    --system \
                    --dest=org.bluez \
                    --object-path=${interface_to_path[${interface}]} \
                    --xml \
                    | xmllint --format - \
                    | grep -v '^ *<node name=".*"/>$' \
                        > specs/$interface.xml
            done
        )
fi

if [ ${GENERATE:-1} = 1 ]; then
    for file in specs/org.bluez.*.xml; do
        interface=$(
            echo $file \
                | sed -e 's:^specs/::' -e 's:[.]xml$::'
        )
        modname=$(
            echo $interface \
                | sed -e 's/^org.bluez.//' \
                | tr '[:upper:]' '[:lower:]'
        )
        dbus-codegen-rust \
            --file=$file \
            --interfaces=$interface \
            --client=nonblock \
            --methodtype=none \
            > generated/$modname.rs
    done
fi
