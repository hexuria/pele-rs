# pele-rs

A Rust port of [Pele](https://github.com/obsidianmd/pele), kepano's Mermaid renderer. Pure
Rust, no dependencies beyond `url`: `pele::render(code)` returns the SVG, and `Scene` holds the
same drawing as a tree with symbolic paints, for a native host to draw.

## Parity contract

For every test input, `render()` returns the SVG upstream Pele returns at the pinned commit,
**byte for byte**, and the same error (code, type, line, column, message, snippet). A type is
ported only when all of its goldens match; other types fail with
`PeleErrorCode::NotYetPorted`. `PORTING.md` has the method and the JavaScript semantics to keep.

Pinned upstream: `36e584d59152181927e0d8bf70fddcd2a5f33d6c` (1.12.0-8-g36e584d).

## Tests

```sh
cargo test                      # golden parity, every ported type
PELE_GOLDEN=flow-edges cargo test --test golden   # inputs whose name contains the text
```

Goldens live in `tests/golden/<type>.jsonl`, one line per input, sorted by name:
`{name, source, mmd, svg | error, width, height, links, type, mermaidType}`. The hand-written
samples are `tests/samples.jsonl`. With the `corpus` feature, `pele::corpus::{samples,
flowchart}` give the inputs as `(name, mmd)` pairs, for downstream crates that test their own
drawing.

## Regenerating goldens

Needs Node. The script clones upstream into the scratch directory, checks out the pinned commit,
builds it, and renders every input:

```sh
node tools/golden.mjs <scratch-dir>         # tests/golden/<type>.jsonl for ported types
node tools/golden.mjs <scratch-dir> --all   # every type, into tests/golden-all/ (gitignored)
```

To port a type, add it to `PORTED` in `tools/golden.mjs` and in `src/core.rs`, and rerun.

## Licence

MIT (`LICENSE`). Upstream's notice and the test inputs' provenance are in `NOTICE`.
