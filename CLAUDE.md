# unifier

Rust-Crate: Modellierungs- und Solver-Framework für Constraint
Satisfaction / Optimization Problems (CSP/COP), aufbauend auf
[`pathwise`](https://github.com/casoon/pathwise)s generischen
Search-/Optimization-Bausteinen. Konzept & Herkunft: `README.md`.
Umsetzungsplan: `plan/`.

Projektname (Repo) und voraussichtlicher crates.io-Paketname sind
identisch, `unifier` — vor Veröffentlichung Verfügbarkeit erneut prüfen.

## Positionierung (siehe `plan/01-concept.md` für Details)

`pathwise` liefert generische Search-/Optimization-Strategien (A*,
Branch & Bound, Local Search, ...) hinter einem `Problem`-Trait.
`unifier` baut die CSP/COP-spezifische Schicht darüber: Variable/Domain,
Constraint-Graph, globale Constraints (`AllDifferent`, `NoOverlap`,
`Cumulative`, ...), Hard/Soft-Scoring, Constraint Propagation. Kein
eigenständiger Ersatz für OR-Tools/MiniZinc/Timefold — diese sind
Referenzen für Konzepte (Global Constraints, Score-Modell), nicht
Vorbild für Code-Übernahme.

## Architektur (Arbeitstitel, siehe `plan/01-concept.md` für Details)

```
DSL              — problem-building surface API (activity/resource/rule)
Constraint Model — Variable, Domain, Constraint-Graph (Hypergraph)
Solver Engine    — Propagation, Backtracking, Branch & Bound,
                    Local Search, LNS
Runtime          — inkrementelles Scoring, Cancellation/Timeout,
                    Anytime-Solving (parallele Suche: später)
```

Problemstruktur ist ein **Constraint-Graph**, kein Baum — der Baum
entsteht erst im Lösungsprozess eines Solvers (Backtracking-Suchbaum,
Branch-and-Bound-Zweige).

Langfristige Stoßrichtung (nicht Teil von 0.1, nur Kontext): `unifier`
ist Phase 3 des in `pathwise`s `plan/01-concept.md` skizzierten Stapels
`pathwise → unifier (constraint solver) → scheduling framework →
Stundenplanung`. Ob das Scheduling-Framework (Activity/Interval/Resource
als eigenständige DSL-Schicht) Teil von `unifier` bleibt oder ein
eigenes Crate wird, ist offen (siehe `plan/01-concept.md`, "Offene
Fragen").

## Arbeitsweise

- Aktueller Stand & nächster Schritt: `plan/00-STATUS.md`.
- Konzept & Scope-Entscheidungen: `plan/01-concept.md`.
- Getroffene Entscheidungen: `plan/DECISIONS.md` — dort nachschlagen,
  bevor offene Fragen neu aufgerollt werden.

## Feste Regeln

- Lizenz: **MIT**, von Anfang an (`Cargo.toml`: `license = "MIT"`).
- Kein `unsafe` ohne expliziten Grund und Kommentar.
- Nichts implementieren, wofür `pathwise`, `std` oder ein etabliertes
  Crate bereits eine gute Lösung bietet.
- Jede öffentliche Funktion/jeder Constraint dokumentiert Komplexität,
  Voraussetzungen und Referenz (Originalpaper/Fachliteratur), nicht nur
  eine Kurzbeschreibung.

## Definition of Done

Noch nicht definiert — Konzeptphase. Wird pro Phase in `plan/0N-*.md`
festgelegt, sobald mit der Implementierung begonnen wird.
