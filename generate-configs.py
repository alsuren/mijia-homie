#!/usr/bin/env python3
import json
from collections import defaultdict
import os


def mutate_grafana_model(model, mac_to_name):
    panel_to_reading_types = defaultdict(set)
    seen = set()

    for panel_idx, panel in enumerate(model["panels"]):
        for target in panel["targets"]:
            if not target["measurement"].startswith("MijiaBridge"):
                continue
            try:
                (_prefix, mac, reading_type) = target["measurement"].split("_")
            except:
                continue

            panel_to_reading_types[panel_idx].add(reading_type)
            seen.add((mac, reading_type))

    for panel_idx, panel in enumerate(model["panels"]):
        reading_types = panel_to_reading_types[panel_idx]
        if len(reading_types) != 1:
            # Don't add readings to a panel with both temperature and humidity,
            # or with no readings of either type.
            continue
        (reading_type,) = reading_types

        for mac, name in mac_to_name.items():
            if (mac, reading_type) not in seen:
                print(f"Adding {mac} {reading_type} {name} to {panel_idx}")
                panel["targets"].append(
                    {
                        "alias": name,
                        "groupBy": [
                            {"params": ["$__interval"], "type": "time"},
                            {"params": ["none"], "type": "fill"},
                        ],
                        "measurement": f"MijiaBridge_{mac}_{reading_type}",
                        "orderByTime": "ASC",
                        "policy": "default",
                        "resultFormat": "time_series",
                        "select": [
                            [
                                {"params": ["value"], "type": "field"},
                                {"params": [], "type": "mean"},
                            ]
                        ],
                        "tags": [],
                    }
                )


def mutate_smarthome_model(model, mac_to_name):
    for mac, name in mac_to_name.items():
        if f"MijiaBridge_{mac}" not in model:
            print(f"adding MijiaBridge_{mac} => {name}")
            model[f"MijiaBridge_{mac}"] = {
                "class": "org.eclipse.smarthome.core.items.ManagedItemProvider$PersistedItem",
                "value": {
                    "groupNames": ["MijiaBridge"],
                    "itemType": "Group",
                    "tags": [],
                    "label": f"{name} sensor",
                },
            }
        for key_suffix, label_suffix in [
            ("Temperature", "temperature"),
            ("Humidity", "humidity"),
            ("BatteryLevel", "battery level"),
        ]:
            key = f"MijiaBridge_{mac}_{key_suffix}"
            value = {
                "class": "org.eclipse.smarthome.core.items.ManagedItemProvider$PersistedItem",
                "value": {
                    "groupNames": [f"MijiaBridge_{mac}"],
                    "itemType": "Number",
                    "tags": [],
                    "label": f"{name} {label_suffix}",
                },
            }
            if key not in model:
                print(f"adding {key} => {name}")
                model[key] = value
            assert model[key] == value


def add_named_sensors_to_grafana(mac_to_name, infilename, outfilename):
    with open(infilename) as f:
        model = json.load(f)

    mutate_grafana_model(model, mac_to_name)

    with open(outfilename, "w") as f:
        json.dump(model, f, indent=2)
        f.write("\n")


def add_named_sensors_to_smarthome(mac_to_name, infilename, outfilename):
    with open(infilename) as f:
        model = json.load(f)

    mutate_smarthome_model(model, mac_to_name)

    with open(outfilename, "w") as f:
        json.dump(model, f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    namesfilename = os.environ.get("NAMES_INPUT", "sensor_names.conf")

    with open(namesfilename) as f:
        mac_to_name = dict(l.strip().replace(":", "").split("=") for l in f)

    add_named_sensors_to_grafana(
        mac_to_name,
        os.environ.get("GRAFANA_INPUT", "grafana-home.json"),
        os.environ.get("GRAFANA_OUPTUT", "new-grafana-home.json"),
    )
    add_named_sensors_to_smarthome(
        mac_to_name,
        os.environ.get("SMARTHOME_INPUT", "org.eclipse.smarthome.core.items.Item.json"),
        os.environ.get(
            "SMARTHOME_OUTPUT", "new-org.eclipse.smarthome.core.items.Item.json"
        ),
    )
