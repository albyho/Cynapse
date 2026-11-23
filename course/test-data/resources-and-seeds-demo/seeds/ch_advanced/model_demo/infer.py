import pathlib
import sys

weights = pathlib.Path(sys.argv[2])
print(weights.exists())
