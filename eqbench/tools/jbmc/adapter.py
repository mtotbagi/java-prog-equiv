"""eqbench adapter for JBMC (https://github.com/diffblue/cbmc), the bounded
model checker for Java bytecode.

    jbmc-eqbench [--budget SECONDS] <old-dir> <new-dir> <method>

Checks the miter of the case (`EQBENCH_MITER`, see eqbench's src/miter.rs):
JBMC explores `eqbench.Miter.check` on every input, its loops and recursion
unwound up to a bound that doubles until it answers:

- every property holds, unwinding assertions included: the loops were fully
  unwound, so the versions agree on every input: `eq`;
- the assertion of the miter fails: a counterexample, `neq`, unless the code
  calls a method JBMC has no body for (e.g. `Math.sin`), whose result it then
  picks freely: the counterexample may not be real, `unknown`;
- only unwinding assertions fail: the bound is too small, it is doubled.

The reason of the answer is printed on the second line.
"""

import argparse
import os
import re
import subprocess
import time
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--models", type=Path, required=True, help="core-models.jar of JBMC")
    # One second below the timeout of `eqbench analyse`, to answer before being killed.
    budget = float(os.environ.get("EQBENCH_TIMEOUT", "10")) - 1
    parser.add_argument("--budget", type=float, default=budget, help="seconds for the whole run")
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("method")
    args = parser.parse_args()

    miter = os.environ.get("EQBENCH_MITER")
    if miter:
        verdict, reason = check(Path(miter) / "classes", args.models, time.monotonic() + args.budget)
    else:
        no_miter = args.old.parent / "no-miter.txt"
        verdict, reason = "unknown", "no miter: " + (no_miter.read_text().strip() if no_miter.exists() else "?")
    print(verdict)
    print(f"JBMC: {reason}")


def check(classes: Path, models: Path, deadline: float) -> tuple[str, str]:
    unwind = 1
    while True:
        left = deadline - time.monotonic()
        if left <= 0:
            return "unknown", f"out of time, unwinding {unwind // 2} times was not enough"
        command = [
            "jbmc", "eqbench.Miter", "--classpath", f"{classes}:{models}",
            "--function", "eqbench.Miter.check",
            "--unwind", str(unwind), "--unwinding-assertions",
            # Runtime exceptions are thrown, and compared by the miter, instead
            # of being checked as properties.
            "--throw-runtime-exceptions",
        ]
        try:
            done = subprocess.run(command, capture_output=True, text=True, timeout=left)
        except subprocess.TimeoutExpired:
            return "unknown", f"out of time while unwinding {unwind} times"
        output = done.stdout + done.stderr
        if "VERIFICATION SUCCESSFUL" in output:
            return "eq", f"proved, unwinding {unwind} times"
        if "VERIFICATION FAILED" not in output:
            return "unknown", "no verdict:\n" + "\n".join(output.strip().splitlines()[-10:])
        failures = [line.strip() for line in output.splitlines() if line.rstrip().endswith(": FAILURE")]
        counterexamples = [f for f in failures if "unwinding assertion" not in f]
        if counterexamples:
            unmodelled = sorted(set(re.findall(r"no body for (?:function|callee) (\S+)", output)))
            if unmodelled:
                return "unknown", "counterexample through methods without a body: " + ", ".join(unmodelled)
            return "neq", "counterexample: " + counterexamples[0]
        unwind *= 2


if __name__ == "__main__":
    main()
