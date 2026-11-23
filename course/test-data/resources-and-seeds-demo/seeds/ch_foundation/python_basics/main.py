import pathlib
import sys

module = pathlib.Path(sys.argv[1])
print(module.exists())
