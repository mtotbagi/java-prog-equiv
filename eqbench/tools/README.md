# Reference tools

Equivalence checkers to evaluate on the 248 method-level pairs with `eqbench analyse`.
Each tool gets its own `tools/<tool>/` directory with a `flake.nix` and an
adapter script that turns its output into `eq` / `neq` / `unknown`, so that the
root flake stays small.

```sh
just eqbench analyse ./eqbench/tools/pasda               # PASDA
just eqbench analyse ./eqbench/tools/pasda --tool ARDiff  # ARDiff, DSE
```

`analyse` builds the flake of the directory with nix before the first case,
then runs the only program of its default package with the given arguments.

| Tool | Status |
|---|---|
| [PASDA, ARDiff, DSE](pasda) | Done, tested on macOS (aarch64), not on Linux yet. ARDiff and DSE are the versions fixed by the authors of PASDA, run from the same build with `--tool`. The adapter makes the compared method the first, static `snippet` method of its class and the other members static, as PASDA requires. They answer `unknown` for `void` methods, non-primitive parameters, methods outside `oldV` / `newV` and pairs PASDA's instrumentation cannot handle (e.g. a callee whose signature changes). `--budget` (default: one second below the timeout of `analyse`) and `--depth` (default 10) can be passed too. |

## Results

First runs on the 248 pairs, 10 s per pair, on an Apple M1 Pro (8 cores, 16 GB).

| Tool | Jobs | Accuracy | F1 | eq precision / recall | neq precision / recall | unknown | Mean time | Wall time |
|---|---|---|---|---|---|---|---|---|
| PASDA | 1 | 58/248 (23.4%) | 0.361 | 0.957 / 0.163 | 0.735 / 0.319 | 176 | 5.1 s | 21 min |
| ARDiff | 6 | 47/248 (19.0%) | 0.303 | 1.000 / 0.148 | 0.643 / 0.239 | 186 | 5.1 s | 3.6 min |
| DSE | 6 | 44/248 (17.7%) | 0.296 | 1.000 / 0.141 | 0.781 / 0.221 | 197 | 4.6 s | 3.2 min |

- Most answers are `unknown`: out of time on the numerical benchmarks (ell,
  bess, gam, ran) and the loops of CLEVER, pairs outside the input format of the
  tools (26 `void` methods, 10 raytrace methods on objects, methods using
  instance members) and pairs PASDA's instrumentation cannot align (REVE).
- Every wrong `neq` (13 for PASDA, 15 for ARDiff, 7 for DSE) is on a CLEVER `Eq`
  pair. For CLEVER, `method name` is the library method that changed (`foo`,
  `lib`) while the pair is equivalent for its client only (`main`, `client`):
  in `CLEVER-Sub-Eq`, `foo(a, b)` returns `a - b` against `b - a`, called as
  `foo(5, 900)` against `foo(900, 5)`. These answers are right for the method;
  `analyse` now passes the client (and the entry point of REVE's `triangular`),
  these results predate it.
- The only wrong `eq` is PASDA on `ran-ranzero-Neq`.
- ARDiff and DSE ran 6 pairs at a time: their times, and so their timeouts, are
  inflated compared to PASDA.

## Shortlist

In integration order: cheapest and most informative first.

| # | Tool | Approach | Can answer | Code |
|---|---|---|---|---|
| 1 | PASDA, ARDiff, DSE, PRV | Symbolic execution (SPF + Z3) of both versions, summary or partition based | eq, neq, maybe, unknown | [glockyco/PASDA](https://github.com/glockyco/PASDA) (MIT) |
| 2 | Differential fuzzing (Jazzer or JQF) | Fuzz a miter `old.m(x) == new.m(x)` | neq, unknown | [Jazzer](https://github.com/CodeIntelligenceTesting/jazzer) (Apache-2.0), [JQF](https://github.com/rohanpadhye/JQF) (BSD-2) |
| 3 | JBMC | Bounded model checking of the miter's bytecode | neq; eq when loops are fully unwound | [diffblue/cbmc](https://github.com/diffblue/cbmc) |
| 4 | GDart, Java Ranger, SWAT | Dynamic symbolic / symbolic / concolic execution of the miter | neq; eq when every path is explored | [GDart](https://github.com/tudo-aqua/gdart), [Java Ranger](https://github.com/vaibhavbsharma/java-ranger), [SWAT](https://github.com/SWAT-project/SWAT) (BSD-2) |
| 5 | Hybrid Eq/Neq | ARDiff / PASDA combined with differential fuzzing | eq, neq, likely | [laboni68/HybridNonEqAnalysis](https://github.com/laboni68/HybridNonEqAnalysis) (no license) |
| 6 | Randoop, EvoSuite | Test generation on the miter | neq, unknown | [Randoop](https://github.com/randoop/randoop) (MIT), [EvoSuite](https://github.com/EvoSuite/evosuite) (LGPL-3.0) |
| 7 | HyDiff | Fuzzing and shadow symbolic execution in parallel | neq, unknown | [yannicnoller/hydiff](https://github.com/yannicnoller/hydiff) (MIT) |

### PASDA, ARDiff, DSE, PRV

- PASDA: Glock, Pichler, Pinzger, JSS 2024 ([arXiv:2311.08071](https://arxiv.org/abs/2311.08071)).
  ARDiff: Badihi et al., ESEC/FSE 2020, by the authors of EqBench.
- One Java 8 build of the PASDA repo gives PASDA and bug-fixed versions of ARDiff,
  DSE and PRV (`PASDA-diff`, `ARDiff-base`, `DSE-base`, `SE-diff`). The original
  ARDiff ([resess/ARDiff](https://github.com/resess/ARDiff), inactive since 2021,
  also ships IMP-S) reports false `NEQ` that PASDA's README documents.
- `CheckPair.sh <dir> [timeout] [depth]` checks one pair. Both versions must be
  `oldV` / `newV` classes with a static `snippet` method of the same signature,
  so the adapter has to bring the method under comparison to that shape.
- Published results are on the ARDiff benchmark, which overlaps EqBench but is
  not the same set of pairs.

### Differential fuzzing

The cheapest baseline. It can only show non-equivalence, which is the hard side
for the symbolic tools.

### JBMC

First of the Java category of [SV-COMP 2026](https://sv-comp.sosy-lab.org/2026/results/results-verified/).
It takes a miter: a `Main` drawing inputs with `Verifier.nondet*()` and asserting
that both versions agree. The `cbmc` package of nixpkgs is built without JBMC
(`-DWITH_JBMC=OFF`), so the flake has to override it or use the SV-COMP archive.

### GDart, Java Ranger, SWAT

Same miter format as JBMC (SV-COMP), so each one is mostly a matter of wrapping
its SV-COMP archive. GDart is second in SV-COMP 2026 Java.

### Hybrid Eq/Neq

Sarker & Bultan, ICST 2025. A fork of the ARDiff / PASDA code with fuzzing added,
and the only tool with published runs on EqBench Java pairs. Research code without
a license.

### Randoop, EvoSuite

Neq-only baselines from test generation. Randoop is in nixpkgs.

### HyDiff

Noller et al., ICSE 2020. Every pair needs a hand-written fuzzing driver and a
merged `change(old, new)` program, hence last.

## Left out

- C only: SymDiff, RVT, REVE / llrêve, CLEVER, ModDiff, DiffKemp, shadow symbolic
  execution, UC-KLEE, PEQcheck.
- Plain SPF, JayHorn, JDart and COASTAL as miter checkers: negative scores in
  SV-COMP 2026 Java, i.e. many wrong answers.
- JLiSA (abstract interpretation, third in SV-COMP 2026 Java): unlikely to prove
  that two different computations agree; cheap to add later through the miter.
- No public code: the original DSE and PRV, DiSE.

## Work shared by the adapters

- **`unknown`**: most tools can answer it (`UNKNOWN`, `MAYBE_EQ`, depth limit...).
  The adapter prints `unknown`, which `analyse` counts as no prediction.
- **Method under comparison**: the tools need it, either by name, as `snippet`,
  or through a miter. `analyse` passes it as third argument: the `method` of
  `J-Desc.json`, or the entry point named by `program name` for CLEVER and REVE
  (`Sub.main` → `main`).
- **JDK**: the JPF-based tools need Java 8 (`jdk8` in nixpkgs).
- **Platform**: the SV-COMP archives and PASDA's Docker image are Linux x86-64.

