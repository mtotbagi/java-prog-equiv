"""eqbench adapter for PASDA (https://github.com/glockyco/PASDA).

    pasda-eqbench [--tool PASDA|ARDiff|DSE|IMP-S|PRV] [--budget SECONDS] [--depth N]
                  <old-dir> <new-dir> <method>

Makes the method under comparison the `public static snippet` method that
PASDA expects, runs the tool in a scratch directory and prints `eq`, `neq` or
`unknown`. The reason of an `unknown` answer is printed on the standard error.

Besides PASDA, its repository ships the versions of ARDiff, DSE, IMP-S and
PRV that its authors fixed or wrote; they run from the same JARs.
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
# For each tool: the arguments of the base runner, the tool of the
# differencing runner when the base one only instruments the pair (the
# partition-based tools), and the name its runs are recorded under.
TOOLS = {
    "PASDA": (["--tool", "P", "--s", "coral"], "PASDA", "PASDA-diff"),
    "PRV": (["--tool", "S", "--s", "coral"], "SE", "SE-diff"),
    "ARDiff": (["--tool", "A", "--s", "coral", "--H", "H123"], None, "ARDiff-base"),
    "DSE": (["--tool", "D", "--s", "coral"], None, "DSE-base"),
    "IMP-S": (["--tool", "I", "--s", "coral"], None, "Imp-base"),
}


class Unknown(Exception):
    """The tool cannot give an answer for this pair."""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--pasda", type=Path, required=True, help="share/pasda of the nix package")
    parser.add_argument("--tool", choices=TOOLS, default="PASDA")
    # One second below the timeout of `eqbench analyse`, to answer before being killed.
    budget = float(os.environ.get("EQBENCH_TIMEOUT", "10")) - 1
    parser.add_argument("--budget", type=float, default=budget, help="seconds for the whole run")
    parser.add_argument("--depth", type=int, default=10, help="depth limit of the symbolic execution")
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("method")
    args = parser.parse_args()

    deadline = time.monotonic() + args.budget
    try:
        old = prepare((args.old / "oldV.java").read_text(), "oldV", args.method)
        new = prepare((args.new / "newV.java").read_text(), "newV", args.method)
        verdict = run(args.pasda, args.tool, old, new, args.depth, deadline)
    except Unknown as reason:
        print(f"unknown: {reason}", file=sys.stderr)
        verdict = "UNKNOWN"
    print(VERDICTS.get(verdict, "unknown"))
    print(f"{args.tool}: {verdict}")


def prepare(source: str, cls: str, method: str) -> str:
    """Makes `method` the `public static snippet` method of class `cls`, and
    the other members of `cls` static so that `snippet` can still use them
    (the client entry points of CLEVER call an instance method)."""
    opening, closing = class_body(source, cls)
    declaration = re.compile(
        rf"^([ \t]*)(?:(?:public|private|protected|static|final|synchronized)\s+)*"
        rf"([\w\[\]<>.]+)\s+{re.escape(method)}\s*\(([^)]*)\)",
        re.M,
    )
    found = next(
        (m for m in declaration.finditer(source, opening, closing) if m.group(2) not in {"return", "new", "else"}),
        None,
    )
    if found is None:
        raise Unknown(f"no method `{method}` with a return type in class {cls}")
    indent, ty, params = found.groups()
    if ty not in PRIMITIVES:
        raise Unknown(f"`{method}` returns {ty}, PASDA compares primitive results only")
    for param in filter(None, (p.strip() for p in params.split(","))):
        param_ty = param.rsplit(None, 1)[0].removeprefix("final ").strip()
        if param_ty not in PRIMITIVES:
            raise Unknown(f"`{method}` takes a {param_ty}, PASDA supports primitive inputs only")

    # PASDA fails on a method without parameters that calls other methods
    # (CLEVER's `main()`), an unused one keeps the semantics.
    if not params.strip():
        params = "int unused"
    # PASDA analyses the first method of the class, so `snippet` goes first.
    end = matching_brace(source, source.index("{", found.end())) + 1
    snippet = f"{indent}public static {ty} snippet({params}){source[found.end() : end]}\n"
    source = source[: found.start()] + source[end:]
    source = source[:opening] + "\n" + snippet + source[opening:]
    if method != "snippet":
        source = re.sub(rf"(?<![\w.]){re.escape(method)}\s*\(", "snippet(", source)
    source = make_static(source, cls)
    return re.sub(r"^\s*package\s+[\w.]+\s*;", f"package {PACKAGE};", source, count=1, flags=re.M)


def class_body(source: str, cls: str) -> tuple[int, int]:
    """Positions after the opening brace of class `cls` and of its closing brace."""
    m = re.search(rf"\bclass\s+{cls}\b[^{{]*\{{", source)
    if m is None:
        raise Unknown(f"no class {cls}")
    return m.end(), matching_brace(source, m.end() - 1)


def matching_brace(source: str, opening: int) -> int:
    """Position of the brace closing the one at `opening`. Braces in strings
    and comments are counted too, none of the supported pairs has any."""
    depth = 0
    for i in range(opening, len(source)):
        depth += {"{": 1, "}": -1}.get(source[i], 0)
        if depth == 0:
            return i
    raise Unknown("unbalanced braces")


# A method or field declaration: modifiers, type, name, then `(`, `=`, `;` or `,`.
MEMBER = re.compile(r"^([ \t]*)((?:(?:public|private|protected|final)\s+)*)([\w\[\]<>.]+\s+\w+\s*[(=;,])", re.M)


def make_static(source: str, cls: str) -> str:
    """Makes every method and field of class `cls` static."""
    opening, end = class_body(source, cls)
    body = source[opening:end]

    def add_static(m: re.Match) -> str:
        # Declarations inside a method body are local variables.
        if body.count("{", 0, m.start()) != body.count("}", 0, m.start()):
            return m.group(0)
        return f"{m.group(1)}{m.group(2)}static {m.group(3)}"

    body = MEMBER.sub(add_static, body)
    body = re.sub(r"^[ \t]*@Override[ \t]*\n", "", body, flags=re.M)  # not allowed on static methods
    body = re.sub(r"(?<![\w.])this\.", f"{cls}.", body)
    return source[:opening] + body + source[end:]


def run(pasda: Path, tool: str, old: str, new: str, depth: int, deadline: float) -> str:
    """Runs `tool` on the two prepared sources, returns its verdict."""
    base_args, diff_tool, recorded_as = TOOLS[tool]
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
                      *base_args, "--b", str(depth), "--t", str(seconds_left(deadline)))
        if diff_tool:
            if not (pair_dir / "instrumented" / f"IDiff{diff_tool}-Parameters.txt").is_file():
                raise Unknown("the instrumentation failed:\n" + tail(output))
            output = java(impl, deadline, DIFF_JAR, pair, diff_tool, str(seconds_left(deadline)), str(depth))
        verdict = read_verdict(impl / "analysis" / "results" / "sqlite.db", recorded_as)
        if verdict is None:
            raise Unknown(f"{tool} recorded no result:\n" + tail(output))
        return verdict


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


def read_verdict(db_path: Path, recorded_as: str) -> str | None:
    with sqlite3.connect(db_path) as db:
        row = db.execute(
            "SELECT result FROM run WHERE id = (SELECT max(run_id) FROM settings WHERE tool = ?)",
            (recorded_as,),
        ).fetchone()
    return row[0] if row else None


def tail(output: str) -> str:
    return "\n".join(output.strip().splitlines()[-15:])


if __name__ == "__main__":
    main()
