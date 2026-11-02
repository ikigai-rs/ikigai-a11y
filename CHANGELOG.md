# Changelog

## 0.4.1

### Added

- `Rgba::parse` reads CSS Color 4 `rgb()` and `rgba()` as well as hex (ledger #2904): the comma
  form (channels all numbers or all percentages, optional alpha) and the space form (mixed
  channels, `none`, alpha after `/`), with alpha as a number or a percentage, out-of-range values
  clamped and fractional channels rounded. The floor pass and `urn:a11y:contrast` therefore measure
  a tint written as `rgba(…)` instead of skipping or refusing it. Named colors, `hsl()`, `var()` and
  `calc()` are still refused. The parse error now names the `rgb()`/`rgba()` forms too.

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
