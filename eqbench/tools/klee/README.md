# KLEE

Symbolic equivalence checker for the C halves of the EqBench pairs.

```sh
just eqbench analyse ./eqbench/tools/klee
```

eqbench passes the version directories (`.../cases/<case-id>/old` and `.../new`),
which now contain `oldV.c` / `newV.c` as well as the Java source. The adapter
also accepts bare `old` / `new` stems (appending `V.c`) and direct `.c` paths.
For one pair it:

1. parses the signature of the requested function in both C files;
2. renames that function in each source, so that old and new can be linked in
   one LLVM module (all other symbols are internalised with `opt`);
3. builds a harness (a generated C `main`) that makes the parameters symbolic,
   calls both versions and asserts that their results agree;
4. runs KLEE on the miter made from the two sources and the harness.

For `main`, pointer parameters such as `char *argv[]` are passed as `NULL`
rather than made symbolic. This assumes the benchmark entry point does not read
`argv`; if it does, KLEE reports a memory error and the adapter prints
`unknown`.

A KLEE assertion failure means `neq`. If KLEE explores every path without one,
the adapter prints `eq`. Anything else — KLEE hitting its time or depth limit,
unsupported types (pointers, structs, `void`), external calls, memory errors,
missing C sources — prints `unknown` with the reason on stderr.

Floating-point concretization warnings are deliberately ignored so the
benchmark's concrete verdict is visible. This means `eq` for a floating-point
program only covers the values KLEE concretized, not all inputs; other KLEE
warnings still produce `unknown`.

The function under comparison must have matching signatures in both versions;
pointers are only supported as `main` parameters passed as `NULL`, and aggregate
results are not supported. Floating-point
results are compared bitwise, with all NaNs treated as equal, so that `-0.0`
and `0.0` are distinguished.

Only `x86_64-linux` is supported. The flake uses the nixpkgs KLEE package and
the matching LLVM 19 toolchain.
