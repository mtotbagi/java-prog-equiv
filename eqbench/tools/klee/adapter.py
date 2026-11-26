"""eqbench adapter for KLEE (https://klee.github.io).

    klee-eqbench [--budget SECONDS] <old-dir> <new-dir> <function>

Compiles both C versions into LLVM bitcode, prefixes the name of the function
under comparison in each version, internalises their other symbols and links
them with a harness:

    klee_make_symbolic(&x, ...);
    if (!same(old(x), new(x))) assert(0);

KLEE then reports a difference as an assertion failure. If it explores every
path without one, the adapter prints `eq`; if KLEE hits one of its limits (or
the bitcode uses an unsupported feature), it prints `unknown`. Reasons for an
`unknown` are written to the standard error. Floating-point concretization
warnings are ignored so that the benchmark's concrete verdict can be seen.
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path

PREFIX = "__eqbench_"

INCLUDES = """
#include <assert.h>
#include <math.h>
#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
"""

INTEGER_TYPES = {
    "char", "signed char", "unsigned char",
    "short", "unsigned short",
    "int", "unsigned int",
    "long", "unsigned long",
    "long long", "unsigned long long",
    "bool",
}

# Canonical spellings of the scalar C types the harness can handle. Anything
# else (pointers, structs, long double, ...) is reported as `unknown`.
TYPES = {
    "void": "void",
    "char": "char", "signed char": "signed char", "unsigned char": "unsigned char",
    "short": "short", "short int": "short",
    "signed short": "short", "signed short int": "short",
    "unsigned short": "unsigned short", "unsigned short int": "unsigned short",
    "int": "int", "signed": "int", "signed int": "int",
    "unsigned": "unsigned int", "unsigned int": "unsigned int",
    "long": "long", "long int": "long",
    "signed long": "long", "signed long int": "long",
    "unsigned long": "unsigned long", "unsigned long int": "unsigned long",
    "long long": "long long", "long long int": "long long",
    "signed long long": "long long", "signed long long int": "long long",
    "unsigned long long": "unsigned long long",
    "unsigned long long int": "unsigned long long",
    "float": "float", "double": "double",
    "bool": "bool", "_Bool": "bool",
}
QUALIFIERS = {"const", "volatile", "restrict", "register", "static", "inline", "extern"}
TYPE_WORDS = set(TYPES) | QUALIFIERS | {"struct", "union", "enum"}


class Unknown(Exception):
    """KLEE cannot decide equivalence for this pair."""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    budget = float(os.environ.get("EQBENCH_TIMEOUT", "10")) - 1
    parser.add_argument("--budget", type=float, default=budget, help="seconds for the whole run")
    parser.add_argument("--depth", type=int, default=500, help="maximum symbolic branches per path")
    parser.add_argument("old", type=Path)
    parser.add_argument("new", type=Path)
    parser.add_argument("function")
    args = parser.parse_args()

    deadline = time.monotonic() + args.budget
    try:
        verdict = decide(args.old, args.new, args.function, args.depth, deadline)
    except Unknown as reason:
        print(f"unknown: {reason}", file=sys.stderr)
        verdict = "unknown"
    print(verdict)


def decide(old_dir: Path, new_dir: Path, function: str, depth: int, deadline: float) -> str:
    old_source = read_c_source(old_dir, "old")
    new_source = read_c_source(new_dir, "new")

    old_sig = signature(old_source, function)
    new_sig = signature(new_source, function)
    if old_sig is None or new_sig is None or old_sig != new_sig:
        raise Unknown(f"the signatures of `{function}` differ or are not supported")
    ret, params = old_sig
    if ret == "void":
        raise Unknown(f"`{function}` returns void, there is no result to compare")
    if any(null for _, _, null in params):
        print(f"note: passing NULL for pointer parameters of `{function}`", file=sys.stderr)

    old_name, new_name = PREFIX + "old_" + function, PREFIX + "new_" + function
    old = prepare_source(old_source, function, old_name)
    new = prepare_source(new_source, function, new_name)
    harness = make_harness(old_name, new_name, ret, params)

    with tempfile.TemporaryDirectory(prefix="klee-eqbench-") as tmp:
        work = Path(tmp)
        (work / "old.c").write_text(old)
        (work / "new.c").write_text(new)
        (work / "harness.c").write_text(harness)

        for name in ("old", "new", "harness"):
            compile_bitcode(work, name, old_name if name == "old" else new_name, deadline)
        link_bitcode(work, deadline)
        output = run_klee(work, depth, deadline)
        return read_verdict(output, work)


def read_c_source(path: Path, version: str) -> str:
    """Finds `oldV.c` / `newV.c` for the path stem given by eqbench.

    Normally `path` is the version directory and the C file is inside it.
    The fallbacks cover tools that pass the bare `old` / `new` stem instead.
    """
    parent = path.parent
    candidates = [Path(str(path) + "V.c"), parent / f"{version}V.c"]
    if path.is_dir():
        candidates = [path / f"{version}V.c", path / f"{version}.c"]
        candidates += sorted(path.glob("*.c")) + [Path(str(path) + "V.c")]
    elif path.is_file():
        candidates = [path, path.with_suffix(".c")]
    if parent.name:
        # Fallback for a dashed case id that was laid out as nested dirs.
        # Only the case id has dashes as separators, not the absolute prefix.
        nested = parent.parent / parent.name.replace("-", "/")
        candidates += [nested / f"{path.name}V.c", nested / f"{version}V.c"]
    for candidate in candidates:
        if candidate.is_file():
            return candidate.read_text(errors="replace")
    raise Unknown(f"no C source for {path}, expected {version}V.c")


def blank_noncode(source: str) -> str:
    """Blanks comments and string literals, preserving offsets and newlines."""
    out = list(source)
    i = 0
    while i < len(source):
        c = source[i]
        if c in "\"'":
            end = i + 1
            while end < len(source):
                if source[end] == "\\":
                    end += 2
                    continue
                if source[end] == c:
                    end += 1
                    break
                end += 1
            for j in range(i, min(end, len(source))):
                if out[j] != "\n":
                    out[j] = " "
            i = end
        elif source.startswith("//", i):
            end = source.find("\n", i)
            end = len(source) if end < 0 else end
            for j in range(i, end):
                out[j] = " "
            i = end
        elif source.startswith("/*", i):
            end = source.find("*/", i + 2)
            end = len(source) if end < 0 else end + 2
            for j in range(i, min(end, len(source))):
                if out[j] != "\n":
                    out[j] = " "
            i = end
        else:
            i += 1
    return "".join(out)


def canonical(words: list[str]) -> str | None:
    words = [w for w in words if w not in QUALIFIERS]
    return TYPES.get(" ".join(words))


def signature(
    source: str, function: str
) -> tuple[str, list[tuple[str, str, bool]]] | None:
    """Return type and parameters of `function` in `source`.

    A parameter is (C declaration, name, pass_null). Only `main` may have
    pointer parameters, which are passed as NULL by the harness.
    """
    code = blank_noncode(source)
    pattern = re.compile(
        rf"(?m)^[ \t]*([A-Za-z_][\w \t\*]*?)\b{re.escape(function)}[ \t]*\(([^)]*)\)[ \t]*\s*\{{"
    )
    m = pattern.search(code)
    if m is None:
        return None
    ret = canonical(m.group(1).split())
    params = parameters(m.group(2), allow_null=function == "main")
    if ret is None or params is None:
        return None
    return ret, params


def parameters(text: str, allow_null: bool = False) -> list[tuple[str, str, bool]] | None:
    text = text.strip()
    if not text or text == "void":
        return []
    result = []
    for index, part in enumerate(split_commas(text)):
        if "*" in part or "[" in part:
            # There is no memory model for arbitrary pointers. The CLEVER
            # `main(int, char *argv[])` cases never read argv, so pass NULL.
            param = null_parameter(part, index) if allow_null else None
            if param is None:
                return None
            result.append(param)
            continue
        words = re.findall(r"[A-Za-z_]\w*", part)
        if not words:
            return None
        name = words[-1] if words[-1] not in TYPE_WORDS else None
        type_words = words[:-1] if name else words
        ty = canonical(type_words)
        if ty is None or ty == "void":
            return None
        result.append((ty, name or f"p{index}", False))
    return result


def null_parameter(part: str, index: int) -> tuple[str, str, bool] | None:
    """Normalize a pointer parameter to `TYPE *name`, to be passed as NULL."""
    match = re.match(
        r"^\s*([A-Za-z_][\w \t]*?)\s*(\**)\s*([A-Za-z_]\w*)\s*(\[\s*\])?\s*$", part
    )
    if match is None:
        return None
    base, stars, name, array = match.groups()
    ty = canonical(base.split())
    depth = len(stars) + (1 if array else 0)
    if ty is None or ty == "void" or depth == 0:
        return None
    return f"{ty} {'*' * depth}{name}", name, True


def split_commas(text: str) -> list[str]:
    parts, depth, start = [], 0, 0
    for i, c in enumerate(text):
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(text[start:i])
            start = i + 1
    parts.append(text[start:])
    return parts


def rename_identifier(source: str, old: str, new: str) -> str:
    """Renames a global function, including its recursive calls."""
    out = []
    i = 0
    while i < len(source):
        c = source[i]
        if c in "\"'":
            end = literal_end(source, i, c)
            out.append(source[i:end])
            i = end
        elif source.startswith("//", i):
            end = source.find("\n", i)
            end = len(source) if end < 0 else end
            out.append(source[i:end])
            i = end
        elif source.startswith("/*", i):
            end = source.find("*/", i + 2)
            end = len(source) if end < 0 else end + 2
            out.append(source[i:end])
            i = end
        elif c.isalpha() or c == "_":
            j = i + 1
            while j < len(source) and (source[j].isalnum() or source[j] == "_"):
                j += 1
            word = source[i:j]
            out.append(new if word == old and not is_member(source, i) else word)
            i = j
        else:
            out.append(c)
            i += 1
    return "".join(out)


def literal_end(source: str, start: int, quote: str) -> int:
    i = start + 1
    while i < len(source):
        if source[i] == "\\":
            i += 2
            continue
        if source[i] == quote:
            return i + 1
        i += 1
    return len(source)


def is_member(source: str, start: int) -> bool:
    """Whether the identifier at `start` is a struct member, not a global."""
    i = start - 1
    while i >= 0 and source[i].isspace():
        i -= 1
    return i >= 0 and (source[i] == "." or (source[i] == ">" and i > 0 and source[i - 1] == "-"))


def prepare_source(source: str, function: str, new_name: str) -> str:
    # The sources ship system includes, but never local ones. Removing them
    # lets the rename touch only the pair's own code, not the C library.
    lines = [line for line in source.splitlines() if not re.match(r"^\s*#\s*include\b", line)]
    return INCLUDES + "\n" + rename_identifier("\n".join(lines), function, new_name) + "\n"


def make_harness(
    old_name: str, new_name: str, ret: str, params: list[tuple[str, str, bool]]
) -> str:
    declarations = ", ".join(decl if null else f"{decl} {name}" for decl, name, null in params)
    arguments = ", ".join("NULL" if null else name for _, name, null in params)
    scalars = [(decl, name) for decl, name, null in params if not null]
    setup = "\n".join(
        f'  klee_make_symbolic(&{name}, sizeof({name}), "{name}");' for _, name in scalars
    )
    values = "\n".join(f"  {decl} {name};" for decl, name in scalars)

    if ret in INTEGER_TYPES:
        equal = "  return a == b;"
    elif ret in ("float", "double"):
        bits = "uint32_t" if ret == "float" else "uint64_t"
        exponent = "0x7f800000U" if ret == "float" else "0x7ff0000000000000ULL"
        mantissa = "0x007fffffU" if ret == "float" else "0x000fffffffffffffULL"
        equal = f"""
  {bits} x, y;
  memcpy(&x, &a, sizeof x);
  memcpy(&y, &b, sizeof y);
  int nan_x = (x & {exponent}) == {exponent} && (x & {mantissa}) != 0;
  int nan_y = (y & {exponent}) == {exponent} && (y & {mantissa}) != 0;
  return x == y || (nan_x && nan_y);"""
    else:
        raise Unknown(f"cannot compare results of type {ret}")

    return f"""{INCLUDES}
extern void klee_make_symbolic(void *, size_t, const char *);
extern {ret} {old_name}({declarations});
extern {ret} {new_name}({declarations});

static int same({ret} a, {ret} b) {{
{equal}
}}

int main(void) {{
  {ret} old_result, new_result;
{values}
{setup}
  old_result = {old_name}({arguments});
  new_result = {new_name}({arguments});
  assert(same(old_result, new_result));
  return 0;
}}
"""


def remaining(deadline: float) -> int:
    left = int(deadline - time.monotonic())
    if left < 1:
        raise Unknown("out of time")
    return left


def command(args: list[str], work: Path, deadline: float) -> str:
    try:
        done = subprocess.run(
            args, cwd=work, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, timeout=remaining(deadline),
        )
    except subprocess.TimeoutExpired:
        raise Unknown(f"{args[0]} timed out") from None
    if done.returncode:
        raise Unknown(f"{args[0]} failed:\n" + tail(done.stdout))
    return done.stdout


def compile_bitcode(work: Path, name: str, public: str, deadline: float) -> None:
    source, bitcode = work / f"{name}.c", work / f"{name}.bc"
    command(
        ["clang", "-emit-llvm", "-c", "-g", "-O0", "-Xclang", "-disable-O0-optnone",
         "-o", str(bitcode), str(source)],
        work, deadline,
    )
    if name != "harness":
        # Every symbol but the function under comparison becomes local, so
        # helpers and globals of the two versions can share their C names.
        command(
            ["opt", "-passes=internalize", f"-internalize-public-api-list={public}",
             "-o", str(bitcode), str(bitcode)],
            work, deadline,
        )


def link_bitcode(work: Path, deadline: float) -> None:
    command(
        ["llvm-link", "-o", str(work / "miter.bc"),
         str(work / "old.bc"), str(work / "new.bc"), str(work / "harness.bc")],
        work, deadline,
    )


def run_klee(work: Path, depth: int, deadline: float) -> str:
    output = work / "klee-out"
    # KLEE's own clock is checked once a second; stop it before the subprocess
    # timeout so that it can still write its statistics and we can parse them.
    seconds = max(1, remaining(deadline) - 2)
    args = [
        "klee", "--solver-backend=z3", f"--max-time={seconds}s",
        f"--max-depth={depth}", f"--output-dir={output}", str(work / "miter.bc"),
    ]
    return command(args, work, deadline)


def read_verdict(output: str, work: Path) -> str:
    logs = [output]
    for path in (work / "klee-out").rglob("*"):
        if path.is_file() and path.name in {"info", "messages.txt"}:
            logs.append(path.read_text(errors="replace"))
    log = "\n".join(logs)
    fp_warning = "silently concretizing (reason: floating point)"
    if fp_warning in log.lower():
        print("note: ignoring KLEE floating-point concretization warnings", file=sys.stderr)
        log = "\n".join(
            line for line in log.splitlines() if fp_warning not in line.lower()
        )
    lowered = log.lower()

    failures = list((work / "klee-out").rglob("*.assert.fail"))
    failures += list((work / "klee-out").rglob("*.assert.err"))
    if failures or re.search(r"assertion fail(?:ed)?", log, re.I):
        return "neq"
    if "done: completed paths" not in lowered:
        raise Unknown("KLEE did not finish the search:\n" + tail(log))
    # KLEE prints statistics even when it stopped early. These markers mean
    # that some paths were left unexplored, so equality is not established.
    for marker in (
        "halting execution", "time expired", "resource limit",
        "klee: error", "klee: warning", "solver error", "unsupported",
        "failure", "failed", "could not",
    ):
        if marker in lowered:
            raise Unknown(f"KLEE stopped early ({marker}):\n" + tail(log))
    partial = re.search(r"partially completed paths\s*=\s*(\d+)", lowered)
    if partial and int(partial.group(1)):
        raise Unknown(f"KLEE left {partial.group(1)} paths unexplored")
    return "eq"


def tail(output: str) -> str:
    return "\n".join(output.strip().splitlines()[-12:])


if __name__ == "__main__":
    main()
