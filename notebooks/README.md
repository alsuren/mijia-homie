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

Make a data export from your influxdb, and rename the files to (notebooks/):

- data/homie_boolean.csv
- data/homie_color.csv
- data/homie_enum.csv
- data/homie_float.csv
- data/homie_integer.csv

## Committing Changes

We use `nbstripout` to strip jupyter notebook cell output when committing to git and diffing.

Run `poetry run nbstripout --install --attributes ../.gitattributes` to get that working if it's not already enabled on your system.
