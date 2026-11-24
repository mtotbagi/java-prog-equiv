"""eqbench adapter for Jazzer (https://github.com/CodeIntelligenceTesting/jazzer),
a coverage-guided fuzzer for the JVM.

    jazzer-eqbench [--budget SECONDS] <old-dir> <new-dir> <method>

Fuzzes the miter of the case (`EQBENCH_MITER`, see eqbench's src/miter.rs)
through `EqbenchFuzzTarget`, which feeds `eqbench.Miter.check` with fuzzed
inputs. Testing can only show non-equivalence:

- the miter's assertion fails on some input: `neq`, the input is printed;
- no difference found within the budget: `unknown`.

The reason of the answer is printed on the second line.
"""

import argparse
import os
import re
import subprocess
import tempfile
import time
from pathlib import Path

# Seconds kept for starting the JVM, out of the fuzzing time.
STARTUP = 5


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target", type=Path, required=True, help="jar of EqbenchFuzzTarget")
    # One second below the timeout of `eqbench analyse`, to answer before being killed.
    budget = float(os.environ.get("EQBENCH_TIMEOUT", "10")) - 1
    parser.add_argument("--budget", type=float, default=budget, help="seconds for the whole run")
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("method")
    args = parser.parse_args()

    miter = os.environ.get("EQBENCH_MITER")
    if miter:
        verdict, reason = fuzz(Path(miter) / "classes", args.target, time.monotonic() + args.budget)
    else:
        no_miter = args.old.parent / "no-miter.txt"
        verdict, reason = "unknown", "no miter: " + (no_miter.read_text().strip() if no_miter.exists() else "?")
    print(verdict)
    print(f"Jazzer: {reason}")


def fuzz(classes: Path, target: Path, deadline: float) -> tuple[str, str]:
    fuzzing = int(deadline - time.monotonic()) - STARTUP
    if fuzzing < 1:
        return "unknown", "no time to fuzz"
    with tempfile.TemporaryDirectory(prefix="jazzer-eqbench-") as tmp:
        command = [
            "jazzer", f"--cp={classes}:{target}", "--target_class=EqbenchFuzzTarget",
            # Assertions on, they are what the miter checks with.
            "--jvm_args=-ea",
            f"-max_total_time={fuzzing}",
            # An input looping for long counts as no finding, not as a crash.
            f"-timeout={fuzzing + STARTUP}",
            f"-artifact_prefix={tmp}/",
        ]
        try:
            # Only stderr is kept: Jazzer reports there. stdout gets what the versions
            # print, millions of lines on the pairs printing an error message (170 MB
            # in 20 s for gam-factln), which would be held in memory.
            done = subprocess.run(command, cwd=tmp, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                  text=True, timeout=max(1.0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            return "unknown", "out of time"
    output = done.stderr
    if "java.lang.AssertionError" in output:
        # Thrown by EqbenchFuzzTarget with the inputs.
        witness = re.search(r"the versions differ on \[.*?\]$", output, re.M)
        return "neq", witness.group(0) if witness else "the versions differ on a fuzzed input"
    runs = [line for line in output.splitlines() if line.startswith("Done ")]
    if done.returncode == 0:
        return "unknown", f"no difference found ({runs[-1] if runs else 'no run count'})"
    return "unknown", "fuzzing failed:\n" + "\n".join(output.strip().splitlines()[-10:])


if __name__ == "__main__":
    main()
