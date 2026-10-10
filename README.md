# ikigai-a11y

Accessibility as a shared, layered, **resolvable** capability for the
[ikigai](https://github.com/ikigai-rs) resource-oriented kernel.

One crate answers *"what are this machine's accessibility settings, for this
application?"* — and carries the contrast machinery every ikigai server and
control plane can reuse instead of re-deriving per front end.

```text
source urn:a11y:config:cms-web                     # the EFFECTIVE merged config
source urn:a11y:presentation:cms-web               # the rendering half of it, ungated
source urn:a11y:config as=text/turtle              # the skolemized graph face
source urn:a11y:contrast from=#323232 on=#2b303b
→ 1.03 fail
```

## Resources

| resource | what it serves | capability |
| --- | --- | --- |
| `urn:a11y:config` | the whole effective config for this machine | `urn:cap:a11y:read` |
| `urn:a11y:config:{app}` | …with `{app}`'s override layer applied | `urn:cap:a11y:read` |
| `urn:a11y:presentation` | the **rendering half**: themes, contrast floors, link underlining | none |
| `urn:a11y:presentation:{app}` | …with `{app}`'s override layer applied | none |
| `urn:a11y:contrast` | the WCAG ratio of two colours, and whether it clears a floor | none |

Faces: `text/plain` (TOML), `as=application/json`, `as=text/turtle` (skolemized,
no blank nodes; the gated view links the files that contributed as
`prov:wasDerivedFrom` plus `ik:sharedLayer`/`ik:appLayer`).

`urn:a11y:contrast` requires no capability because it is arithmetic over two
colours the caller already holds; declaring one would make the manifold
under-offer. `urn:a11y:config` requires one because it states whether the person
at the keyboard needs reduced motion and larger text — assistive-technology
information about a human being, not decoration.

## ★ Two views, and the rule that goes with them

**Deriving an artifact — a stylesheet, a palette, a rendered page? Read
`urn:a11y:presentation`, or `ikigai_a11y::load::presentation`.** It hands over
the configured themes, both contrast floors and whether links are underlined, and
it cannot hand over anything about the person reading. No capability is needed,
because none of it is about them.

`urn:a11y:config` and `ikigai_a11y::load::complete` serve the whole thing,
`motion.reduce` and `text.scale` included. Two callers are entitled to that: the
thing serving the gated resource, and a process configuring itself for the user
it runs as.

### What the capability actually protects

**The resource, not the files.** A capability is checked when the kernel resolves
an IRI. Nothing checks one when a linked library reads `a11y.toml` with
`std::fs` — and nothing could: a consumer that wanted the bytes could open the
file itself whether or not this crate existed. On disk the config is protected by
filesystem permissions and by nothing else.

That is the shape of the thing rather than a hole in it. `urn:cap:a11y:read` is
the only fence that exists where it matters most — an agent's action manifold, a
peer across a transport, an MCP projection — and an in-process consumer is
already running as the user whose home directory it is reading.

So the crate does what a library *can* do, which is least privilege: it makes the
ungated path lead somewhere harmless. `ikigai-browse` 0.2.12 is why this is
written down. It needs two theme names and a floor, and got `motion.reduce` and
`text.scale` as well — not because it wanted them, but because there was one
function. Three more front ends were queued to copy that call.

### Why a second IRI rather than a face

`as=` cannot carry authority. `requires` is per-action, so an action whose
capability requirement depends on which arguments arrive must declare either the
union (over-demanding for the ungated call) or the intersection (a lie for the
gated one). Authority that differs becomes an **action** that differs — the same
reason `urn:a11y:contrast` takes an explicit `min` instead of reading the
configured floor. `urn:a11y:presentation` is this crate's
`urn:personal:availability`: a separate name for the minimized view at the lower
authority.

The two graph faces therefore carry **different subjects**. On
`urn:a11y:presentation` an absent `ik:reduceMotion` means *withheld*; on
`urn:a11y:config` it means *unstated*. Sharing a subject would merge those the
moment a reader unioned the graphs.

### Where the line is drawn

| | | |
| --- | --- | --- |
| `theme.light` / `theme.dark` | how the artifact is drawn | ungated |
| `contrast.min` / `contrast.min_large` | the deployment's floor | ungated |
| `text.underline_links` | WCAG 1.4.1 posture — about the artifact | ungated |
| `motion.reduce` | this person needs animation suppressed | gated |
| `text.scale` | this person needs text at 1.75× | gated |
| contributing file paths | absolute paths in someone's home | gated |

`A11y::presentation` is the only projection, so **a new key is withheld by
default** and publishing it takes a deliberate line of code — the safe direction
for the default to point.

The residual, stated rather than papered over: a `contrast.min` of 7.0 is a weak
signal that *someone* here wants AAA. It is machine-wide rather than per-person,
it is the operator's posture about the artifact, and anything that renders the
page has to be able to read it. That is a judgement, not an oversight.

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

## Mounting: the config home is stated, not sniffed

`space()` mounts the endpoints over **this machine's** config home — the sugar a
host configuring itself from its own environment wants. `space_with(handle)`
mounts them over a home the caller states:

```rust
use ikigai_a11y::{space_with, A11yHandle};
use std::sync::Arc;

// This machine, with this binary's override layer.
let mounted = space_with(Arc::new(A11yHandle::ambient(Some("cms-web".into()))));

// Or a home the caller owns — a fixture, or another user's config.
let over_fixture = space_with(Arc::new(A11yHandle::new(Some(home), None)));
```

The home is taken **at construction**, never per request: it is a fact about the
mount, and one process legitimately has two answers. The handle holds the *home*
rather than a parsed config, because the golden thread's contract is that cutting
it recomputes — a handle that cached the config would serve the one the process
started with forever.

An `{app}` in the IRI still wins over the mount's own: `urn:a11y:config:cms-web`
names its own resource, and a mount's default applies only where the IRI names
none. No config home at all is a legal under-configured state (`Option`, never a
guessed directory); it becomes an error at the resolution that needed it.

Why it matters beyond tidiness: while the endpoints read `$HOME` themselves, this
crate's own endpoint suite passed 58/58 against three different configs —
including one whose `a11y.toml` did not parse — because no test owned the file
its values came from. See `ikigai-core/docs/design/hermetic-endpoint-tests.md`.

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

### ★ Proving the config reached the output: re-run the pass over it

This is the general answer to *"how does a downstream crate prove that its
`a11y.toml` actually reached the artifact it generated?"* — and it was invented
twice, independently, in one day. It is one line:

> **Re-run `apply_floor` at the configured `min` over the generated output, and
> assert `lifted.is_empty()`.**

The pass is idempotent, so a second pass over output that was already produced at
that floor finds nothing to lift. If it *does* lift something, the floor never
arrived — the config was not read, or was read and then not threaded through to
the generator. There is no third explanation, which is what makes it an assertion
rather than a heuristic.

**Assert on `lifted`, not on `clears_floor()`.** That is the whole reason the split
between `lifted` and `unrepaired` exists here. `unrepaired` holds rules that are
below the floor and *cannot* be repaired from the theme's own palette — a rule that
repaints its own background is the usual case, since it changes what its text sits
on. Those are a fact about the theme, not evidence that the config failed to
arrive, and counting them would make the assertion fail for a reason the consumer
cannot fix.

`ikigai-browse`'s `a_raised_contrast_floor_reaches_the_stylesheet` is the worked
example: it writes `min = 7.0` into a tempdir `a11y.toml`, resolves the stylesheet
through the kernel, and re-runs the pass over each scheme block. Two disciplines in
it are what make such a test durable rather than a maintenance tax:

- **Read the ground and foreground back out of the generated output** rather than
  restating them in the test. A test that hard-codes the colours it expects stops
  testing the config the moment the theme is retuned, and does so silently.
- **Assert the mechanism, not a colour count.** Then add the anti-vacuity control:
  check that the sheet generated at the *default* floor does **not** clear the
  raised one. Without it the test passes forever the day a theme starts clearing
  7:1 on its own, and stops proving that anything reached anything. `ikigai-browse`
  spells that failure out in the assertion message, so the next person is told what
  to do (raise the floor, or name a control theme) instead of deleting the test.

## Cacheability

Both views are `.cacheable()` with a golden thread on **every candidate
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
stylesheets recompute, nothing polls. **This module declares the threads and
cuts none of them** — it has no watcher — so without that wiring an edit is
served stale from the cache. `tests/conformance.rs` pins both halves: the
stale read after an edit with no cut, and the recompute after cutting exactly
the name `load::threads_in` returns.

## Conformance

The module **passes
[`ikigai-conformance`](https://github.com/ikigai-rs/ikigai-conformance)** with
no opt-outs: `tests/conformance.rs` mounts `space_with` over a fresh temporary
config home it seeds (a shared layer plus one app override, so the graph face
is exercised with both layer roles) and runs every check — ArgSpec
completeness, declared = enforced via `urn:cap:a11y:read`, the skolemized
Turtle face against `ikigai-vocab`, cacheability, pipeline citizenship. The
config views are declared `cacheable` and held to it; they are deliberately
**not** declared `pure` — their results depend on the files — so a cached
config with no golden thread but its own name would fail the test.
`urn:a11y:contrast` is declared both, being arithmetic over its arguments. One
check is skipped, and the report says so: `NAMES`, because the three ids are
live MCP tool names and are renamed in one coordinated pass across every module.

**The space has no name of its own; the host names it.** Both constructors are
declared host-named to `SPACE-NAME`: `space_with(handle)` serves whatever home it
was handed, and `space()` reads this process's config home while it builds, so
neither is configuration-free and neither may claim `urn:iki:space:a11y`.

## Layout

| module | | |
| --- | --- | --- |
| `color` | WCAG luminance and ratio, alpha-composited | pure, wasm |
| `config` | schema, key-wise merge, both views, the three faces | pure, wasm |
| `css` | the contrast-floor pass over generated CSS | pure, wasm |
| `load` | the layered read from a stated (or ambient) config home | native |
| `themes` | theme name ⇄ `syntect` theme, turnkey CSS | feature `themes` |

The default build is wasm-clean; `themes` is off by default because `two-face`
carries ~2MB of embedded theme data a browser consumer of the pure halves has no
use for.

## Licence

MIT OR Apache-2.0.
