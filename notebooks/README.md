# Jupyter Notebooks for analyzing your sensor data

## Installing dependencies

To get started, from this directory, run:

```bash
poetry install
```

and then run this command to find out where your python interpreter lives:

```bash
poetry run which python
```

In VSCode:

- Click View -> Command Pallette (cmd+shift+p) and type `Python: Select Interpreter`
- Click Enter interpreter path...
- Paste the path from above.

## Getting the data

Export the data from InfluxDB, e.g.:

```bash
for measurement in boolean color enum float integer; do curl "http://localhost:8086/query?db=$DATABASE&u=$USERNAME&p=$PASSWORD&chunked=true" -H "Accept: application/csv" --data-urlencode "q=SELECT * FROM $measurement" > homie_$measurement.csv; done
```

Put the files under `notebooks/data/`.

## Committing Changes

We use `nbstripout` to strip jupyter notebook cell output when committing to git and diffing.

Run `poetry run nbstripout --install --attributes ../.gitattributes` to get that working if it's not already enabled on your system.

## Looking at the data

The data will be split into different csv files, split by different data types.According to your setup, there will be up to 5 files:
- homie_boolean: contains all metrics stored as booleans (smart lights)
- homie_enum
- homie_color: contains rgb values for the smart lights
- homie_float: contains all metrics stored as floats (temperature)
- homie_integer: contains all metrics stored as integers (humidity %, battery level %)

Here, we want to focus on the csvs containing floats and integers, as they contain the temperature/humdity data. Useful columns:
- time: since epoch (unix epoch 1970). pandas handles this for us.
device_id
- device_name: only use data with device containing raspberry pi or cottage pi
- node_id: mac address of the sensor
- node_type: =="Mijia sensor" to select only the temperature/humidity sensor data
- node_name: nickname for the sensor (e.g., "living room")

There are between 4 and 10 data points per sensor per minute, depending on how often a sensor gets polled (~ 10K data points in a 24h period for a given sensor)