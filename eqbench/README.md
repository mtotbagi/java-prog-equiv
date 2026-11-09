# eqbench

CLI around the Java programs of [EqBench](https://github.com/shrBadihi/EqBench)
(`src_data`): 272 pairs of programs, each one identified as `<bench>-<prog>-<Eq|Neq>`.

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
correct predictions: 147/272 (54.0%)
time: total 2.64 s, mean 9.7 ms, max 12.4 ms (optimization-theta-Neq)
errors: 0 timeouts, 0 crashes, 0 invalid outputs, 0 not built
```
