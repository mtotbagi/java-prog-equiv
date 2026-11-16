# Reference tools

Equivalence checkers to evaluate on the 248 method-level pairs with `eqbench analyse`.
Each tool will get its own `tools/<tool>/` directory with a `flake.nix` and an
adapter script that turns its output into `eq` / `neq`, so that the root flake
stays small.

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
  or through a miter. It is the `method` of `J-Desc.json`, not passed to the tool yet.
- **Miter equality**: NaN, `-0.0`, exceptions, arrays and objects must be compared
  the same way for every tool.
- **JDK**: the JPF-based tools need Java 8 (`jdk8` in nixpkgs).
- **Platform**: the SV-COMP archives and PASDA's Docker image are Linux x86-64.
