# eqbench

CLI around the Java programs of [EqBench](https://github.com/shrBadihi/EqBench)
(`src_data`): 248 pairs of methods, each one identified as `<bench>-<prog>-<Eq|Neq>`.
The 24 whole-program pairs (no `J-Desc.json`) are left out.

## Commands

| Command | Description |
|---|---|
| `list` | List every case |
| `details <id>` | Show the metadata of a case and the diff between `oldV.java` and `newV.java` |
| `build` | Compile every case to bytecode (`javac`), then to JSON (`jvm2json`) in `target/eqbench` |
| `clean` | Remove `target/eqbench` |
| `analyse <cmd…>` | Run `<cmd…> <old-dir> <new-dir>` on every case, the tool must print `eq` or `neq` |

## Example

```sh
$ just eqbench build
$ just eqbench analyse ./my-tool
...
correct predictions: 135/248 (54.4%)
time: total 11.49 s, mean 46.3 ms, max 56.3 ms (ran-poidev-Eq)
errors: 0 timeouts, 0 crashes, 0 invalid outputs, 0 not built
```
