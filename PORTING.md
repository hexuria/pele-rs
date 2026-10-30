# Porting Pele to Rust

The crate ports [obsidianmd/pele](https://github.com/obsidianmd/pele) at the commit pinned in
`tools/golden.mjs` and `NOTICE`. The contract is byte parity: for every input, `render()` gives
the SVG upstream gives, and the same error (code, type, line, column, message, snippet).

## How the port is built

- **Modules mirror upstream's `src/`** file for file, and each names the file it ports.
  Helpers that are not upstream files: `js.rs` (JS semantics), `scene.rs` and `svg/write.rs`
  (the drawing as a tree), `util/json.rs` (`JSON.parse`).
- **Renderers port upstream's string building line for line.** Upstream assembles SVG from
  strings and tests those strings with regexes (`rounded()`, `setsFill()`, `textStyle()`,
  `RE_USED` in `root.ts`), so building a tree in parallel would mean re-deriving every one of
  those checks. Instead `Scene::parse` reads the SVG back into an element tree (attributes in
  order, values as written) and `svg::write` writes the same bytes; the golden test checks the
  round trip on every golden. Paints stay symbolic in the tree (`Paint::Var`, `Paint::Literal`).
- **Options are upstream's defaults**, as the Node helper used them: no `maxWidth` (so
  `fit-width.ts`'s `tighten`/`turnToFit`/`capWidth` are identities and were not ported), 16px
  text, the metrics measurer (never a canvas or an injected measurer: it changes geometry), no
  icon resolver and no math resolver (`text/icons.rs`, `text/math.rs` are stubs; a label never
  holds an icon or math span), default `idPrefix`, links and images allowed with the default
  schemes, `rel="noopener"`.
- **Types not ported** fail with `PeleErrorCode::NotYetPorted` after detection, so the host can
  draw them another way. `detect.rs` has all 37 rows.

## JavaScript semantics to keep

- **UTF-16.** `.length`, `charCodeAt`, error lines/columns and the edits that map positions back
  through preprocessing all count UTF-16 code units. `preprocess.rs`, `errors.rs` and the
  flowchart lexer work on `Vec<u16>`; token texts become `String` afterwards. Label layout reads
  chars, which only differs for characters outside the BMP, and no decision there depends on
  them. The source limit (50,000) and output limit (4,000,000) are UTF-16 lengths.
- **Numbers.** Coordinates go through `num()` (two decimals, its own rounding); a few values are
  written with plain `String(n)` (`viewBox`/`width`/`height`, `font-size` of edge labels, the
  `translate()` of stacked documents): use `js::num_str`, which prints exponents as JS does.
  `Math.round` rounds halves up (`js::round`), not away from zero.
- **Sorting.** `Array.prototype.sort` is stable; a comparator's NaN counts as 0; `a - b || c`
  falls through on 0 *and* NaN (`js::or`, `js::ord`). Keep every tie-break.
- **Insertion order.** `Map`/`Set` iterate in insertion order (`assignTracks`' `byRank`,
  `resolveStyle`'s property maps, class lists). Use ordered containers there. Plain objects put
  integer-like keys first; nothing ported so far depends on that (`util/yaml.rs` keeps insertion
  order).
- **Whitespace.** `\s` and `trim()` are JS's set (no U+0085, with U+FEFF): `js::is_space`,
  `js::trim`. Rust's `str::trim` differs.
- **Regexes are matched by hand**, with their backtracking worked out. Ones that bit:
  - `RE_NODIR` `(?:\r?\n)*\s*\n`: the whitespace run up to its *last* newline.
  - `RE_DIRECTION` `.*direction\s+TB[^\n]*`: `.` stops at `\r`, U+2028/9, `[^\n]` does not.
  - `RE_LINK` `--+[-xo>]`: the last dash can be the terminator.
  - `RE_DIRECTIVE`: group 3 (`\w+`) wins over group 4 when the value starts with a word char;
    only group 4 is parsed as `init` JSON.
  - `RE_PIECE` (label wrapping): opening brackets are also CJK, so `[OPEN]*[CJK]` backtracks.
  - `RE_SCHEME` `/^.+(:|&colon;)/gim`: multiline, greedy to the last colon of the first line
    that has one.
  - `RE_BREAK` `/<br\s*\/?>|\\n|\n/gi`: case-insensitive, so `\N` breaks a line too.
  - `RE_HTML_ENTITIES` in `url.ts`: `String.fromCharCode("x41")` is `"\0"`.
- **Aliasing.** Upstream mutates shared arrays: `updateLink` hands one style array to every
  position and appends `fill:none` once; `position()` sorts `succE`/`predE` in place and later
  passes read the sorted order; `compound.ts` caches layered results and returns the same
  objects. `addSubGraph` compares `idText === titleText` by identity (a header with no
  `[title]`).
- **`Math.hypot`** is V8's: scaled by the larger value, with Kahan summation (`js::hypot`).
  Rust's `f64::hypot` can differ in the last bit, which iterative layouts (venn) carry forward.
- **Default `sort()`** on strings compares UTF-16 code units, not code points (`venn::sort16`).
- **`\W` and `.length`** count UTF-16 units: `label.replace(/\W/g, '_')` gives two underscores
  for a character outside the BMP (usecase generated ids).
- **Character classes:** `[\w-:]` is `\w`, `-` and `:` (Annex B: a class escape cannot start a
  range), not the range `-` to `:` (architecture icons).
- **Object key order:** `JSON.parse` puts integer-like keys first; usecase JSON nodes keep each
  key where it was first written, with its last value, which is what upstream's second walk
  over the text recovers.
- **`new URL()`** is the `url` crate (WHATWG): `https://example.com` becomes
  `https://example.com/`. `decodeURIComponent` throws on bad UTF-8 or stray `%`.
- **`JSON.parse`** for `%%{init}%%`: strict JSON after `'` → `"`; anything else is ignored.
- **Past the end of a string** `charCodeAt` is NaN. Most tests on it are false (`isSpace`,
  `isWord`, `=== c`), but not all: the sequence lexer's `has()` falls through to its non-ASCII
  branch and answers true there. The lexers read past the end as `u32::MAX` and keep each
  test's answer for NaN.
- **`Math.sign(0)` is 0**; `f64::signum(0.0)` is 1. Renderers that scale by a sign use a local
  `sign()`.
- **`Object.assign(existing, node)`** copies properties set to `undefined` too, but not ones
  that were never set: state's graph keeps a node's `parentId`, `position` and `noteFor` only
  where the new node lacks the property.
- **`String.prototype.substring`** swaps its ends when the start is past the end (class
  members); `lastIndexOf(s, pos)` clamps a negative `pos` to 0 (er's generic types).
- **Object identity.** Sequence boxes and er/class/state maps compare entries by identity
  (`run.box === box`, `Map` keyed by object); the port keeps indexes into the db's vectors.

## Adding a diagram type

- **Registry.** `diagrams/mod.rs` has `Diagram` (upstream's `types.ts` interface: `type_`,
  `section`, `keep_comments`, `parse`, `render`) and `ALL`, the registered types. A type adds
  its `pub static DIAGRAM` there and its name to `PORTED` in `core.rs` (a test checks the two
  agree) and in `tools/golden.mjs`, then reruns the script for its goldens.
- **What `core.rs` already does**: `keepComments` (the text with `%%` lines kept, and source
  positions mapped through the same edits), `lineOffset` (the front matter lines removed),
  `useMaxWidth: false` in the type's config section (`section`, else its type name) giving
  `RenderOptions { responsive: false }`, and error tagging. Math is off (no resolver), so a
  type's `math: true` changes nothing.
- **Shared modules, ported:** `diagrams/common/*` (`tokens`, `acc_descr`, `scan`, `outline`,
  `literal_label`, `fit`, `fit_text`, `fit_width`, `generics`, `caseless`, `icon`),
  `svg/title.rs`'s `with_title`, `text/label.rs`'s `label_line`/`line_middle`/`with_heights`/
  `has_math` (with no math: no `heights`), `layout/lanes.rs`, `layout/components.rs`.
  - `common/tokens.rs`: upstream's sticky regular expressions are `Matcher` closures
    (`Rc<dyn Fn(&[u16], usize) -> Option<usize>>`); the shared terminals (`acc_descr()`,
    `title()`, `title_before(rest)`, `string()`, `yaml()`, `directive()`, `keyword(word)`, …)
    are functions that build a `TokenType`. `longer` holds indexes into the same table.
    `title_before` takes the lookahead as a predicate.
  - `common/fit_width.rs`: with no `maxWidth`, every fitting step draws once at the natural
    width; the functions keep upstream's shape so renderers port line for line.
- **Fitting with no `maxWidth`:** `turnToFit`, `tighten` and `fitWidth` draw once at the natural
  width, so the down-running drawings (`verticalCauses`, `verticalEventModel`, folded railroad
  sequences) are never reached and are not ported.
- **Flowchart variants:** `flowchart/render.rs` has the `FlowVariant` hook (`prepare`, `layout`,
  `draw_group`, `filled_box`) for swimlane, agentflow, eventmodeling and wardley.
- The Scene and Rox's `app/src/mermaid/draw.rs` cover the 11 elements plus `title`, `desc`,
  `image` and a nested `svg` (icons). Other types may write attributes `draw.rs` does not
  know yet: extend its "every input translates" test (it uses `pele::corpus`) as they land.
