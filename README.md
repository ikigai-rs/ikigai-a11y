# ikigai-a11y

Accessibility as a shared, layered, **resolvable** capability for the
[ikigai](https://github.com/ikigai-rs) resource-oriented kernel.

One crate answers *"what are this machine's accessibility settings, for this
application?"* — and carries the contrast machinery every ikigai server and
control plane can reuse instead of re-deriving per front end.

```text
source urn:a11y:config:cms-web                     # the EFFECTIVE merged config
source urn:a11y:config as=text/turtle              # the skolemized graph face
source urn:a11y:contrast from=#323232 on=#2b303b
→ 1.03 fail
```

## Resources

| resource | what it serves | capability |
| --- | --- | --- |
| `urn:a11y:config` | the effective config for this machine | `urn:cap:a11y:read` |
| `urn:a11y:config:{app}` | …with `{app}`'s override layer applied | `urn:cap:a11y:read` |
| `urn:a11y:contrast` | the WCAG ratio of two colours, and whether it clears a floor | none |

Faces: `text/plain` (TOML), `as=application/json`, `as=text/turtle` (skolemized,
no blank nodes, with `ik:layer` links to the files that contributed).

`urn:a11y:contrast` requires no capability because it is arithmetic over two
colours the caller already holds; declaring one would make the manifold
under-offer. `urn:a11y:config` requires one because it states whether the person
at the keyboard needs reduced motion and larger text — assistive-technology
information about a human being, not decoration.

## The layering

Lowest precedence first: **built-in defaults → `a11y.toml` → `{app}.a11y.toml` →
explicit args**, from the ikigai config home
(`$XDG_CONFIG_HOME/ikigai`, else `~/.config/ikigai`).

`app` is the **process's** name — `cms-web`, `dev-server`, `web` — not a
module's. Each binary declares its own.

**The merge is key-wise.** A layer states only its differences:

```toml
# ~/.config/ikigai/a11y.toml — the posture every front end shares
[contrast]
min = 7.0            # this deployment holds itself to AAA body text
min_large = 4.5

[theme]
dark = "Nord"
```

```toml
# ~/.config/ikigai/cms-web.a11y.toml — one front end differs on one key
[theme]
dark = "Dracula"
```

cms-web gets Dracula **and** the 7.0 floor. Wholesale replacement — the easy
accidental implementation — would silently drop the operator's floor.

## The schema

```toml
[theme]
light = "InspiredGithub"      # any of the 32 embedded themes, either spelling
dark  = "Base16OceanDark"     # ("Base16OceanDark" or "base16-ocean.dark")

[contrast]
min       = 4.5               # WCAG AA body text (7.0 = AAA)
min_large = 3.0               # WCAG AA large text and UI components

[motion]
reduce = true                 # OMIT to defer to the OS — see below

[text]
scale           = 1.0         # 0.5 – 4.0
underline_links = true        # colour alone must not carry meaning (WCAG 1.4.1)
```

Three things are deliberately not what a first sketch reaches for:

- **two contrast floors, not one.** WCAG does not have a single number: body text
  is 4.5:1 while large text and non-text UI are 3.0:1. One number either
  over-constrains headings or drops body text below AA.
- **`motion.reduce` is three-valued.** Omitted means *unstated* — emit
  `@media (prefers-reduced-motion: reduce)` and let the OS answer the question the
  user already answered once. A default of `false` would be the config home
  claiming this user does not need reduced motion.
- **unknown keys are errors.** A misspelled `[contast]` that silently does
  nothing is the same defect as a misspelled theme that silently renders the
  default: the operator asked for something and got something else without being
  told. Unknown theme names and out-of-range numbers are errors for the same
  reason.

## The contrast-floor pass

A syntax theme is a palette plus a ground, and some of its scope colours do not
clear the floor against that ground. Which ones is a property of the *theme*, so
it can be computed once rather than patched per scope as each is noticed.

```rust
let pass = ikigai_a11y::themes::theme_css(EmbeddedThemeName::Base16OceanDark, "hl-", 4.5);
pass.css          // the stylesheet, sub-floor colours lifted
pass.lifted       // what changed, and from what ratio
pass.unrepaired   // what could NOT be fixed in-palette — reported, not hidden
```

The repair colour is **the theme's own default foreground** — the one colour a
theme guarantees is legible on its own ground. Nothing is invented, no new hue
enters the palette, and a rule the pass cannot fix in-palette is left exactly as
the theme wrote it and reported. The pass is idempotent.

`cargo run --features themes --example theme_survey` prints the state of all 32
embedded themes at a given floor.

## Cacheability

`urn:a11y:config` is `.cacheable()` with a golden thread on **every candidate
file** — including ones that do not exist yet, so creating an override
invalidates a cached answer computed before it existed.

Reading a file is not a reason to be uncacheable; it is a reason to declare a
thread. Effective expiry propagates from dependencies, so an uncacheable config
silently un-caches every stylesheet that joins it. Measured with
`cargo run --release --example cache_measure`:

```text
                                   uncacheable   cacheable
urn:a11y:config (reads 1-2 files)         41.5         1.0    42×
a consumer joining it                    376.0         1.2   308×

cached read 1.1µs → first read after cutting the a11y.toml thread 390.3µs
```

A host that watches the config home and cuts the threads from
`ikigai_a11y::load::threads` gets the good version: edit `a11y.toml`, derived
stylesheets recompute, nothing polls.

## Layout

| module | | |
| --- | --- | --- |
| `color` | WCAG luminance and ratio, alpha-composited | pure, wasm |
| `config` | schema, key-wise merge, the three faces | pure, wasm |
| `css` | the contrast-floor pass over generated CSS | pure, wasm |
| `load` | the layered read from the config home | native |
| `themes` | theme name ⇄ `syntect` theme, turnkey CSS | feature `themes` |

The default build is wasm-clean; `themes` is off by default because `two-face`
carries ~2MB of embedded theme data a browser consumer of the pure halves has no
use for.

## Licence

MIT OR Apache-2.0.
