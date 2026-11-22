# eqbench

CLI around the Java programs of [EqBench](https://github.com/shrBadihi/EqBench)
(`src_data`): 248 pairs of methods, each one identified as `<bench>-<prog>-<Eq|Neq>`.
The 24 whole-program pairs (no `J-Desc.json`) are left out.

## Commands

| Command | Description |
|---|---|
| `list` | List every case |
| `details <id>` | Show the metadata of a case and the diff between `oldV.java` and `newV.java` |
| `build` | Compile every case to bytecode (`javac`), then to JSON (`jvm2json`); `target/eqbench/cases/<id>/<old\|new>` holds the source and the JSON of each version |
| `clean` | Remove `target/eqbench` |
| `analyse [--timeout S] [--jobs N] [--output FILE [--resume]] <cmd…>` | Run `<cmd…> <old-dir> <new-dir> <method>` on every case (10 s each, one at a time by default), the tool must print `eq`, `neq` or `unknown`; report accuracy, F1 (mean of `eq` and `neq`) and timings, and write one JSON line per case to `FILE` (`--resume` keeps the cases it already holds) |

## Example

```sh
$ just eqbench build
$ just eqbench analyse ./my-tool              # or a tool of tools/, e.g. ./eqbench/tools/pasda
...
accuracy  135/248 (54.4%)
f1        0.352 (mean of eq and neq)
          eq   precision 0.544  recall 1.000  f1 0.705
          neq  precision -      recall 0.000  f1 0.000
unknown   0/248
time      total 11.98 s, mean 48.3 ms, max 83.8 ms (ej_hash-testCollision3-Eq), wall 12.04 s with 1 job
errors    0 timeouts, 0 crashes, 0 invalid outputs, 0 not built
```
