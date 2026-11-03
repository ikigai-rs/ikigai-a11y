# Changelog

## Unreleased

### Fixed

- The floor pass reads the `background` shorthand (ledger #1652, point 4): its color is the rule's
  ground, and whichever of `background` and `background-color` applies decides it, so
  `background: #eeeeee; color: #ffffff` is no longer judged on the page ground. A shorthand with
  an image, a gradient or a second layer has no one color to measure and is listed by `skipped()`,
  whose `Skip::property` can now be `background`.
- A rule that paints a ground and sets no color is judged with the theme's foreground, and listed
  in `unrepaired` when that fails. Before, nothing measured it, so `clears_floor()` was true for
  SolarizedDark while its `.hl-invalid` text sat at 3.16:1. A `clears_floor()` that was true can be
  false now; the two default themes are unaffected, and no rule is rewritten differently.

## 0.5.0

A minor, not a patch: an `a11y.toml` whose `contrast.min` is below 4.5 (or `contrast.min_large`
below 3.0) loaded before and is refused now, so a consumer on `^0.4` keeps the old loader until
it moves its pin. No other change in this release breaks a caller.

### Added

- `Rgba::parse` reads CSS Color 4 `rgb()` and `rgba()` as well as hex (ledger #2904): the comma
  form (channels all numbers or all percentages, optional alpha) and the space form (mixed
  channels, `none`, alpha after `/`), with alpha as a number or a percentage, out-of-range values
  clamped and fractional channels rounded. The floor pass and `urn:a11y:contrast` therefore measure
  a tint written as `rgba(…)` instead of skipping or refusing it. Named colors, `hsl()`, `var()` and
  `calc()` are still refused. The parse error now names the `rgb()`/`rgba()` forms too.
- `FloorPass::skipped()` and `css::Skip` (ledger #1652, point 3): every value the floor pass needed
  and could not read (a named color, `hsl()`, `var()`, a value with `!important`, and the
  unreadable `background-color` of a rule that sets a `color`) is listed instead of being dropped
  silently. The pass still leaves each one exactly as written.

### Changed

- `FloorPass::clears_floor()` is false while `skipped()` lists anything: a value the pass could
  not read is not proved to clear the floor.
- `contrast.min` below 4.5 and `contrast.min_large` below 3.0 (WCAG AA) are refused at load, with
  an error that names the WCAG floor (ledger #1652, point 7). Before, any floor from 1.0 to 21.0
  was accepted. An operator may still raise either floor up to 21.0.
- `urn:a11y:contrast` refuses a `min` outside 1 to 21 (and `NaN` or infinity) with
  `InvalidArgument`, naming the WCAG floors. A `min` below 4.5 stays legal there: 3.0 is the
  large-text and non-text floor.
- `urn:a11y:contrast` never shows a failing ratio at the floor it failed (ledger #1652, point 8):
  the text face uses two decimals, or as many more as it takes to keep the number on the side of
  the floor its verdict is on, so #d010d0 on white reads `4.4998 fail`, not `4.50 fail`. The JSON
  face's `ratio` is the same number.

### Fixed

- The floor pass reads the declaration CSS applies when a rule declares `color` or
  `background-color` more than once (ledger #2908): the last `!important` one if there is one,
  otherwise the last. Before, it read the first, so `color: #c0c5ce; color: #bf616a` was judged by
  the overridden color and the one that renders was never lifted, and the converse rewrote a dead
  declaration. `skipped()` lists the applied value by the same reading.
- `Rgba::parse` strips exactly one leading `#` (ledger #2907): `##fff` was read as white and is
  now an error.
- American spelling throughout the code, tests, endpoint descriptions and README ("color", not
  "colour"); the description text of `urn:a11y:contrast`'s `on` argument changes accordingly.
- The two README tables with empty header cells now have headers.

## 0.4.0

### Changed

- **Breaking:** move to `ikigai-core` 0.2.0, the streams release (ledger #2261).
  Consumers must use core 0.2 because core types appear in this crate's public API.
- Upgrade `ikigai-conformance` to 0.11.0 and require every check to run, including
  the default DATATYPES and FAILURES checks.

## 0.3.0

### Renamed

**Breaking** for anything that matches on a description id or on the MCP tool
name derived from it. The naming convention in `ikigai-core` (a resource name is
a short `kebab-case` noun, and the id is what the MCP projection turns into an
agent's tool name) now holds for every endpoint here, so the conformance suite's
`NAMES` check runs instead of being skipped (ledger #11).

| old id | new id | IRIs (unchanged) |
| --- | --- | --- |
| `a11yConfig` | `a11y-config` | `urn:a11y:config`, `urn:a11y:config:{app}` |
| `a11yPresentation` | `a11y-presentation` | `urn:a11y:presentation`, `urn:a11y:presentation:{app}` |
| `a11yContrast` | `a11y-contrast` | `urn:a11y:contrast` |

No IRI changed, so every `source urn:a11y:…` request resolves exactly as before.
An id is not an IRI and nothing resolves by it: what moves is the catalog label
and the MCP tool name.
