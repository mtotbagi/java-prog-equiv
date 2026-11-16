"""eqbench adapter for PASDA (https://github.com/glockyco/PASDA).

    pasda-eqbench [--budget SECONDS] [--depth N] <old-dir> <new-dir> <method>

Makes the method under comparison the `public static snippet` method that
PASDA expects, runs the two runners of PASDA in a scratch directory and
prints `eq`, `neq` or `unknown`. The reason of an `unknown` answer is printed
on the standard error.
"""

import argparse
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

# Neutral package, so that the tool never sees the Eq / Neq of the case.
PACKAGE = "eqbench.pair"
PRIMITIVES = {"int", "long", "short", "byte", "char", "boolean", "float", "double"}
BASE_JAR = "build/libs/ARDiff-base-1.0-SNAPSHOT-all.jar"
DIFF_JAR = "build/libs/ARDiff-diff-1.0-SNAPSHOT-all.jar"
VERDICTS = {"EQ": "eq", "NEQ": "neq"}


class Unknown(Exception):
    """PASDA cannot give an answer for this pair."""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--pasda", type=Path, required=True, help="share/pasda of the nix package")
    parser.add_argument("--budget", type=float, default=9.0, help="seconds for the whole run")
    parser.add_argument("--depth", type=int, default=10, help="depth limit of the symbolic execution")
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("method")
    args = parser.parse_args()

    deadline = time.monotonic() + args.budget
    try:
        old = prepare((args.old / "oldV.java").read_text(), "oldV", args.method)
        new = prepare((args.new / "newV.java").read_text(), "newV", args.method)
        verdict = run(args.pasda, old, new, args.depth, deadline)
    except Unknown as reason:
        print(f"unknown: {reason}", file=sys.stderr)
        verdict = "UNKNOWN"
    print(VERDICTS.get(verdict, "unknown"))
    print(f"PASDA: {verdict}")


def prepare(source: str, cls: str, method: str) -> str:
    """Makes `method` the `public static snippet` method of class `cls`.

    Nothing else changes: as in PASDA's own benchmark, the method must not
    use the instance members of its class, otherwise PASDA fails on the pair.
    """
    if not re.search(rf"\bclass\s+{cls}\b", source):
        raise Unknown(f"no class {cls}")
    declaration = re.compile(
        rf"^([ \t]*)(?:(?:public|private|protected|static|final|synchronized)\s+)*"
        rf"([\w\[\]<>.]+)\s+{re.escape(method)}\s*\(([^)]*)\)",
        re.M,
    )
    found = next((m for m in declaration.finditer(source) if m.group(2) not in {"return", "new", "else"}), None)
    if found is None:
        raise Unknown(f"no method `{method}` with a return type")
    indent, ty, params = found.groups()
    if ty not in PRIMITIVES:
        raise Unknown(f"`{method}` returns {ty}, PASDA compares primitive results only")
    for param in filter(None, (p.strip() for p in params.split(","))):
        param_ty = param.rsplit(None, 1)[0].removeprefix("final ").strip()
        if param_ty not in PRIMITIVES:
            raise Unknown(f"`{method}` takes a {param_ty}, PASDA supports primitive inputs only")

    source = f"{source[: found.start()]}{indent}public static {ty} snippet({params}){source[found.end() :]}"
    if method != "snippet":
        source = re.sub(rf"(?<![\w.]){re.escape(method)}\s*\(", "snippet(", source)
    return re.sub(r"^\s*package\s+[\w.]+\s*;", f"package {PACKAGE};", source, count=1, flags=re.M)


def run(pasda: Path, old: str, new: str, depth: int, deadline: float) -> str:
    """Runs PASDA on the two prepared sources, returns its verdict."""
    with tempfile.TemporaryDirectory(prefix="pasda-eqbench-") as tmp:
        impl = Path(tmp) / "implementation"
        # PASDA wants an Eq or NEq directory, it stores it but never reads it.
        pair = "../benchmarks/eqbench/pair/NEq"
        pair_dir = Path(tmp) / "benchmarks" / "eqbench" / "pair" / "NEq"
        pair_dir.mkdir(parents=True)
        (pair_dir / "oldV.java").write_text(old)
        (pair_dir / "newV.java").write_text(new)
        setup_implementation(pasda, impl)

        output = java(impl, deadline, BASE_JAR,
                      "--path1", f"{pair}/oldV.java", "--path2", f"{pair}/newV.java",
                      "--tool", "P", "--s", "coral", "--b", str(depth), "--t", str(seconds_left(deadline)))
        if not (pair_dir / "instrumented" / "IDiffPASDA-Parameters.txt").is_file():
            raise Unknown("the instrumentation failed:\n" + "\n".join(output.strip().splitlines()[-15:]))
        java(impl, deadline, DIFF_JAR, pair, "PASDA", str(seconds_left(deadline)), str(depth))
        return read_verdict(impl / "analysis" / "results" / "sqlite.db")


def setup_implementation(pasda: Path, impl: Path) -> None:
    """The working directory of the runners: read-only files are linked to
    the nix store, the ones they write to are copied."""
    impl.mkdir()
    for name in ("build", "jpf-git", "src", "site.properties"):
        (impl / name).symlink_to(pasda / name)
    # Instrumented classes are compiled into target/classes.
    for directory, _, files in os.walk(pasda / "target"):
        target = impl / "target" / Path(directory).relative_to(pasda / "target")
        target.mkdir(parents=True, exist_ok=True)
        for name in files:
            shutil.copyfile(Path(directory) / name, target / name)
    results = impl / "analysis" / "results"
    results.mkdir(parents=True)
    with sqlite3.connect(results / "sqlite.db") as db:
        db.executescript((pasda / "analysis" / "create-tables.sql").read_text())
    # On Linux, PASDA runs the z3 binary of its working directory.
    z3 = shutil.which("z3")
    if z3:
        (impl / "z3").symlink_to(z3)


def java(cwd: Path, deadline: float, jar: str, *args: str) -> str:
    try:
        done = subprocess.run(["java", "-jar", jar, *args], cwd=cwd, stdout=subprocess.PIPE,
                              stderr=subprocess.STDOUT, text=True, timeout=deadline - time.monotonic())
    except subprocess.TimeoutExpired:
        raise Unknown("out of time") from None
    return done.stdout


def seconds_left(deadline: float) -> int:
    left = int(deadline - time.monotonic())
    if left < 1:
        raise Unknown("out of time")
    return left


def read_verdict(db_path: Path) -> str:
    with sqlite3.connect(db_path) as db:
        row = db.execute(
            "SELECT result FROM run WHERE id = (SELECT max(run_id) FROM settings WHERE tool = 'PASDA-diff')"
        ).fetchone()
    if row is None or row[0] is None:
        raise Unknown("PASDA recorded no result")
    return row[0]


if __name__ == "__main__":
    main()
