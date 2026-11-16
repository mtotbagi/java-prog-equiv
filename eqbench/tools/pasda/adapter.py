"""eqbench adapter for PASDA (https://github.com/glockyco/PASDA).

    pasda-eqbench [--budget SECONDS] [--depth N] <old-dir> <new-dir> <method>

Reads oldV.java and newV.java from the two case directories, brings the
method under comparison to the shape PASDA expects (a public static `snippet`
method of classes oldV / newV), runs the two runners of PASDA in a scratch
directory and prints `eq`, `neq` or `unknown`. The reason of an `unknown`
answer is printed on the standard error.
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
MODIFIERS = r"(?:(?:public|private|protected|static|final|synchronized|strictfp)\s+)*"
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


# Rewriting of the sources.


def prepare(source: str, cls: str, method: str) -> str:
    """Makes `method` the `public static snippet` method of class `cls`.

    Every other member of `cls` becomes static too, so that `snippet` can
    still use them.
    """
    source = re.sub(r"^\s*package\s+[\w.]+\s*;", f"package {PACKAGE};", source, count=1, flags=re.M)
    start, end = class_body(source, cls)
    body = source[start:end]

    member = re.compile(rf"^([ \t]*)({MODIFIERS})([\w\[\]<>.]+\s+)?(\w+)\s*(\(|=|;|,)", re.M)
    found = None
    out, last = [], 0
    for m in member.finditer(body):
        if depth_at(body, m.start()) != 0:
            continue
        indent, modifiers, ty, name, after = m.groups()
        if ty is None:
            if after == "(" and name == cls:
                continue  # constructor of the class, unused once static
            continue
        ty = ty.strip()
        if ty in {"return", "new", "throw", "else"}:
            continue
        if after == "(" and name == method:
            found = (ty, body[m.end() : body.index(")", m.end())])
            replacement = f"{indent}public static {ty} snippet("
        elif "static" in modifiers.split():
            continue
        else:
            replacement = f"{indent}{modifiers}static {ty} {name}{' ' if after != '(' else ''}{after}"
        out.append(body[last : m.start()])
        out.append(replacement)
        last = m.end()
    out.append(body[last:])
    body = "".join(out)

    if found is None:
        raise Unknown(f"no method `{method}` with a return type in class {cls}")
    ty, params = found
    if ty not in PRIMITIVES:
        raise Unknown(f"`{method}` returns {ty}, PASDA compares primitive results only")
    for param in filter(None, (p.strip() for p in params.split(","))):
        param_ty = param.rsplit(None, 1)[0].replace("final ", "").strip()
        if param_ty not in PRIMITIVES:
            raise Unknown(f"`{method}` takes a {param_ty}, PASDA supports primitive inputs only")

    if method != "snippet":
        body = re.sub(rf"(?<![\w.]){re.escape(method)}\s*\(", "snippet(", body)
    body = re.sub(r"(?<![\w.])this\.", f"{cls}.", body)
    return source[:start] + body + source[end:]


def class_body(source: str, cls: str) -> tuple[int, int]:
    """Span of the body of class `cls`, between its braces."""
    m = re.search(rf"\bclass\s+{cls}\b[^{{]*\{{", source)
    if m is None:
        raise Unknown(f"no class {cls}")
    depth = 0
    for i in code_positions(source, m.end() - 1):
        if source[i] == "{":
            depth += 1
        elif source[i] == "}":
            depth -= 1
            if depth == 0:
                return m.end(), i
    raise Unknown(f"unbalanced braces in class {cls}")


def depth_at(body: str, position: int) -> int:
    """Brace depth of `position` in a class body (0: member level)."""
    depth = 0
    for i in code_positions(body, 0, position):
        if body[i] == "{":
            depth += 1
        elif body[i] == "}":
            depth -= 1
    return depth


def code_positions(text: str, start: int, end: int | None = None):
    """Positions of `text` outside comments, strings and characters."""
    end = len(text) if end is None else end
    i = start
    while i < end:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            i = end if i == -1 else i
        elif text.startswith("/*", i):
            i = text.find("*/", i + 2)
            i = end if i == -1 else i + 2
        elif c in "\"'":
            j = i + 1
            while j < end and text[j] != c:
                j += 2 if text[j] == "\\" else 1
            i = j + 1
        else:
            yield i
            i += 1


# Running PASDA.


def run(pasda: Path, old: str, new: str, depth: int, deadline: float) -> str:
    """Runs PASDA on the two prepared sources, returns its verdict."""
    with scratch_directory() as root:
        impl = root / "implementation"
        # PASDA wants an Eq or NEq directory, it stores it but never reads it.
        pair = root / "benchmarks" / "eqbench" / "pair" / "NEq"
        relative_pair = "../benchmarks/eqbench/pair/NEq"
        pair.mkdir(parents=True)
        (pair / "oldV.java").write_text(old)
        (pair / "newV.java").write_text(new)
        setup_implementation(pasda, impl)

        output = java(impl, deadline, BASE_JAR,
                      "--path1", f"{relative_pair}/oldV.java", "--path2", f"{relative_pair}/newV.java",
                      "--tool", "P", "--s", "coral", "--b", str(depth), "--t", str(seconds_left(deadline)))
        if not (pair / "instrumented" / "IDiffPASDA-Parameters.txt").is_file():
            raise Unknown("the instrumentation failed:\n" + tail(output))
        java(impl, deadline, DIFF_JAR, relative_pair, "PASDA", str(seconds_left(deadline)), str(depth))
        return read_verdict(impl / "analysis" / "results" / "sqlite.db")


def setup_implementation(pasda: Path, impl: Path) -> None:
    """The working directory of the runners: read-only files are linked to
    the nix store, the ones they write are copied."""
    impl.mkdir(parents=True)
    for name in ("build", "jpf-git", "src", "site.properties"):
        (impl / name).symlink_to(pasda / name)
    copy_tree(pasda / "target", impl / "target")
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


def tail(text: str, lines: int = 15) -> str:
    return "\n".join(text.strip().splitlines()[-lines:])


def copy_tree(src: Path, dst: Path) -> None:
    """Copies `src` without its permissions: files of the nix store are read-only."""
    for directory, _, files in os.walk(src):
        target = dst / Path(directory).relative_to(src)
        target.mkdir(parents=True, exist_ok=True)
        for name in files:
            shutil.copyfile(Path(directory) / name, target / name)


class scratch_directory:
    """A temporary directory named after the process.

    eqbench kills the tool on timeout, so directories of dead processes are
    removed on the next run instead of on exit only.
    """

    def __enter__(self) -> Path:
        parent = Path(tempfile.gettempdir()) / "pasda-eqbench"
        parent.mkdir(exist_ok=True)
        for leftover in parent.glob("run-*"):
            pid = leftover.name.removeprefix("run-")
            if pid.isdigit() and not alive(int(pid)):
                shutil.rmtree(leftover, ignore_errors=True)
        self.path = parent / f"run-{os.getpid()}"
        shutil.rmtree(self.path, ignore_errors=True)
        self.path.mkdir()
        return self.path

    def __exit__(self, *_) -> None:
        shutil.rmtree(self.path, ignore_errors=True)


def alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


if __name__ == "__main__":
    main()
