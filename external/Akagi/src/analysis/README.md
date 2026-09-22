# `src/analysis/` — mahjong analysis engine

Port of the [EndlessCheng/mahjong-helper](https://github.com/EndlessCheng/mahjong-helper)
`util/` package (Go) to Rust. Reuses `riichienv-core` for tiles, shanten, yaku,
and score primitives; implements the discard search, improves, agari-rate,
tenpai-rate, and risk engine on top.

## How to extend

### Adding a new analysis output

1. Add the field to `result::AnalysisResult` (or `Hand13Result` /
   `Hand14Result` once they exist).
2. Compute it in the relevant module (e.g. risk numbers in
   `risk/`, hand metrics in `analysis/`).
3. Wire it through `analyze()` in `mod.rs`.

### Adding a new risk-correction term (Phase 3)

1. Implement the per-tile correction inside `risk/base.rs` as a
   method on `RiskTiles34`, mirroring the Go `FixWith*` family.
2. Add fixtures from the corresponding Go test (`risk_base_test.go`).

### Updating riichienv-core

The crate is touched in only three files: `tile.rs`, `shanten.rs`,
and (Phase 2 onward) `score.rs`. If `riichienv-core` changes its
shanten / parser API, only those modules need to follow.

## Files

| File | Purpose |
|---|---|
| `tile.rs` | `Tile34` newtype + mjai-string conversion |
| `hand.rs` | `PlayerInfo34` input + `Meld34` |
| `shanten.rs` | Wraps `riichienv_core::shanten` |
| `waits.rs` | Per-tile wait enumeration (any shanten) |
| `agari_rate.rs` | Per-wait agari probability |
| `score.rs` | Point expectation via `riichienv_core::HandEvaluator` |
| `improves.rs` | 13-tile aggregate analysis (`analyze_13`) |
| `search.rs` | 14-tile discard search (`analyze_14`) |
| `result.rs` | Serializable result types |
| `tenpai_rate.rs` | Open-hand tenpai-rate estimate |
| `risk/wall.rs` | NC / OC / DNC wall analysis |
| `risk/base.rs` | Per-opponent deal-in risk + corrections |
| `risk/mod.rs` | Mixed risk across opponents + best-defence pick |
| `data/agari.rs` | `agariMap` + suji classification |
| `data/point.rs` | Ron-point baselines |
| `data/risk.rs` | `RiskRate[turn][type]` + dora multipliers |
| `data/tenpai.rs` | `tenpaiRate[melds][turn][tedashi]` |
| `snapshot_adapter.rs` | `GameStateSnapshot + seat → PlayerInfo34` |
| `runner.rs` | Subscribe + analyze + broadcast on `AnalysisBus` |

## License note

Algorithms and numerical tables are facts and are not protected by
copyright. The mahjong-helper Go source is consulted for behaviour
but not copied verbatim. Tests reuse named fixture hands.
