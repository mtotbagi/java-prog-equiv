"""eqbench adapter for Java Ranger (https://github.com/vaibhavbsharma/java-ranger),
Symbolic PathFinder with path merging.

    java-ranger-eqbench [--budget SECONDS] <old-dir> <new-dir> <method>

Runs Java Ranger on the miter of the case (`EQBENCH_MITER`, see eqbench's
src/miter.rs) as in SV-COMP: `eqbench.Miter.main`, whose inputs come from
Java Ranger's own `Verifier` and are symbolic, with its SV-COMP configuration.
The search depth is bounded, and the bound doubles until:

- an `AssertionError` is reachable: the versions differ on some input, `neq`;
- no error and the depth limit never reached: every path was explored, `eq`;
- out of time: `unknown`.

The reason of the answer is printed on the second line.
"""

import argparse
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# Java Ranger's configuration for SV-COMP (jr-sv-comp in its archive), whose
# doubles range over [-2^31, 2^31].
CONFIG = """\
target=eqbench.Miter
classpath={java_ranger}/java-ranger/build/classes,{classes}
symbolic.dp=z3bitvector
symbolic.bvlength=64
symbolic.min_int=-2147483648
symbolic.max_int=2147483647
symbolic.min_double=-2147483648.0
symbolic.max_double=2147483648.0
symbolic.fp=true
symbolic.lazy=on
symbolic.jrarrays=true
veritestingMode=5
recursiveDepth=200
listener=.symbc.VeritestingListener
singlePathOptimization=true
targetProperty=validity
search.depth_limit={depth}
"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--java-ranger", type=Path, required=True, help="the unpacked SV-COMP archive")
    # One second below the timeout of `eqbench analyse`, to answer before being killed.
    budget = float(os.environ.get("EQBENCH_TIMEOUT", "10")) - 1
    parser.add_argument("--budget", type=float, default=budget, help="seconds for the whole run")
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("method")
    args = parser.parse_args()

    miter = os.environ.get("EQBENCH_MITER")
    if miter:
        verdict, reason = check(args.java_ranger, Path(miter) / "classes", time.monotonic() + args.budget)
    else:
        no_miter = args.old.parent / "no-miter.txt"
        verdict, reason = "unknown", "no miter: " + (no_miter.read_text().strip() if no_miter.exists() else "?")
    print(verdict)
    print(f"Java Ranger: {reason}")


def check(java_ranger: Path, classes: Path, deadline: float) -> tuple[str, str]:
    lib = java_ranger / "java-ranger" / "lib"
    env = dict(os.environ, TARGET_CLASSPATH_WALA=f"{classes}/")
    # The x86-64 Z3 of the archive loads its own libz3 by bare name.
    env["DYLD_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"] = str(lib)
    with tempfile.TemporaryDirectory(prefix="java-ranger-eqbench-") as tmp:
        # Java Ranger writes its witnesses to the working directory.
        work = Path(tmp)
        (work / "site.properties").write_text(
            f"jpf-core = {java_ranger}/jpf-core\n"
            f"jpf-symbc = {java_ranger}/java-ranger\n"
            "extensions=${jpf-core},${jpf-symbc}\n"
        )
        depth = 16
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                return "unknown", f"out of time, a search depth of {depth // 2} was not enough"
            (work / "config.jpf").write_text(CONFIG.format(java_ranger=java_ranger, classes=classes, depth=depth))
            command = [
                "java", "-Xmx1024m", "-ea", f"-Djava.library.path={lib}",
                "-jar", str(java_ranger / "jpf-core" / "build" / "RunJPF.jar"),
                f"+site={work / 'site.properties'}", str(work / "config.jpf"),
            ]
            try:
                done = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, timeout=left)
            except subprocess.TimeoutExpired:
                return "unknown", f"out of time at search depth {depth}"
            output = done.stdout + done.stderr
            errors = [line for line in output.splitlines() if line.startswith("error #")]
            if any("AssertionError" in e for e in errors):
                return "neq", f"assertion violated at search depth {depth}"
            if errors:
                return "unknown", errors[0]
            if "no errors detected" not in output:
                return "unknown", "no verdict:\n" + "\n".join(output.strip().splitlines()[-10:])
            if "depth limit reached" not in output:
                return "eq", f"every path explored, search depth {depth}"
            depth *= 2


if __name__ == "__main__":
    main()
