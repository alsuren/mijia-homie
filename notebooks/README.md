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
