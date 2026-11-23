#!/usr/bin/env node
// Runs Python with the first interpreter that actually works.
//
// Windows resolves `python` to a Microsoft Store alias that exits 9009 when the
// real interpreter is missing, and some machines only have `python3` or a
// conda install. Validating each candidate keeps `pnpm verify` from failing
// with an exit code nobody can explain.
import { spawnSync } from "node:child_process";

const candidates = [
  process.env.PYTHON,
  "python3",
  "python",
  "py",
].filter(Boolean);

function usable(command) {
  const probe = spawnSync(command, ["--version"], { stdio: "ignore" });
  return probe.status === 0;
}

const interpreter = candidates.find(usable);
if (!interpreter) {
  console.error(
    `No usable Python interpreter. Tried: ${candidates.join(", ")}. ` +
      "Set PYTHON to the interpreter path, for example " +
      "PYTHON=C:\\path\\to\\python.exe",
  );
  process.exit(2);
}

const result = spawnSync(interpreter, process.argv.slice(2), {
  stdio: "inherit",
  env: {
    ...process.env,
    PYTHONIOENCODING: process.env.PYTHONIOENCODING ?? "utf-8",
  },
});
if (result.error) {
  console.error(`Failed to run ${interpreter}: ${result.error.message}`);
  process.exit(2);
}
process.exit(result.status ?? 1);
