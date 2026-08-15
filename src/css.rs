//! The contrast-floor pass: repair a generated stylesheet **in its own palette**.
//!
//! A syntax theme is a palette plus a ground. Some of its scope colours clear the
//! contrast floor against that ground and some do not — and which is which is a
//! property of the theme, not of the code being highlighted, so it can be
//! computed once and repaired once for every theme, rather than patched by hand
//! per scope as it is noticed.
//!
//! The repair colour is **the theme's own default foreground**. That is the one
//! colour a theme guarantees is legible on its own ground, so lifting to it
//! invents nothing: a scope that was going to be hard to read renders as ordinary
//! text in the theme's own hand, which is what an unstyled scope already does.
//! No new hue enters the palette and no other rule changes.
//!
//! This generalizes a supplement `ikigai-browse` currently hand-writes
//! (`DARK_PARAMETER`, added 2026-08-15 after `.hl-variable` was found rendering
//! parameters at 3.23:1 on base16-ocean.dark). That constant is one instance of
//! the pass's output; with the pass, it should become unnecessary.
//!
//! Pure text-in, text-out — no syntect, no theme objects — so it compiles to
//! wasm and works on a stylesheet from any source. A caller with a real
//! `syntect::highlighting::Theme` gets the ground and foreground handed to it by
//! the [`themes`](crate::themes) face.

use crate::color::{ratio, Rgba};

/// One rule the pass changed.
#[derive(Clone, PartialEq, Debug)]
pub struct Lift {
    /// The rule's selector, as written.
    pub selector: String,
    /// The colour it declared.
    pub from: Rgba,
    /// The colour it now declares — always the theme's own foreground.
    pub to: Rgba,
    /// The ratio the original colour achieved against its ground.
    pub ratio: f64,
}

/// A rule below the floor that the pass did **not** rewrite, because the
/// theme's own foreground would not clear the floor on that rule's ground
/// either — so there is no in-palette repair, and inventing one is not this
/// pass's business.
#[derive(Clone, PartialEq, Debug)]
pub struct Finding {
    /// The rule's selector, as written.
    pub selector: String,
    /// The colour it declares.
    pub color: Rgba,
    /// The ratio it achieves against its ground.
    pub ratio: f64,
}

/// The result of a floor pass: the rewritten stylesheet and what it changed.
#[derive(Clone, PartialEq, Debug)]
pub struct FloorPass {
    /// The stylesheet with sub-floor foreground colours lifted.
    pub css: String,
    /// Every rule that was lifted, in source order.
    pub lifted: Vec<Lift>,
    /// Every rule below the floor that could not be repaired in-palette — left
    /// exactly as the theme wrote it, and reported rather than hidden. A caller
    /// that wants a guarantee rather than a best effort checks this is empty.
    pub unrepaired: Vec<Finding>,
    /// The repair colour's OWN ratio against the page ground.
    ///
    /// Normally high — a theme's foreground is legible on its own ground by
    /// construction — but a theme where it is below the floor cannot be repaired
    /// from its own palette at all, and the honest answer for such a theme is to
    /// not offer it at that floor rather than to invent a colour for it.
    pub foreground_ratio: f64,
}

impl FloorPass {
    /// Whether the repair colour itself clears `min` — i.e. whether the pass
    /// could do its job at all on this ground.
    pub fn repairable(&self, min: f64) -> bool {
        self.foreground_ratio >= min
    }

    /// Whether every rule now clears the floor: the pass repaired what it found
    /// and left nothing behind.
    pub fn clears_floor(&self) -> bool {
        self.unrepaired.is_empty()
    }
}

/// Lift every `color:` declaration whose contrast against its ground is below
/// `min` to `foreground`, leaving everything else — including every declaration
/// that already clears the floor — byte-for-byte unchanged.
///
/// A rule that declares its own `background-color` is measured against **that**,
/// not against the page ground: a rule which repaints its own ground has changed
/// what its text sits on. Translucent colours are composited before measuring
/// (see [`crate::color`]).
pub fn apply_floor(css: &str, ground: Rgba, foreground: Rgba, min: f64) -> FloorPass {
    let replacement = foreground.to_css();
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut lifted = Vec::new();
    let mut unrepaired = Vec::new();

    for block in declaration_blocks(css) {
        let body = &css[block.body.clone()];
        // The rule's own ground, if it repaints one.
        let local_ground = declaration(body, "background-color")
            .and_then(|d| Rgba::parse(&body[d.value.clone()]).ok())
            .map(|bg| bg.over(ground))
            .unwrap_or(ground);
        let Some(color) = declaration(body, "color") else {
            continue;
        };
        let Ok(current) = Rgba::parse(&body[color.value.clone()]) else {
            // An unparseable colour is left alone: this pass repairs contrast, it
            // does not police syntax, and a value it cannot read is one it cannot
            // reason about either.
            continue;
        };
        let achieved = ratio(current, local_ground);
        if achieved >= min {
            continue;
        }
        // The repair must actually repair. On a rule that repaints its own,
        // low-contrast ground — or one that already IS the foreground — lifting
        // changes the bytes without changing the legibility, and a pass that did
        // it anyway would not be idempotent and would report a fix that isn't.
        if ratio(foreground, local_ground) < min {
            unrepaired.push(Finding {
                selector: css[block.selector.clone()].trim().to_string(),
                color: current,
                ratio: achieved,
            });
            continue;
        }
        let value = block.body.start + color.value.start..block.body.start + color.value.end;
        edits.push((value, replacement.clone()));
        lifted.push(Lift {
            selector: css[block.selector.clone()].trim().to_string(),
            from: current,
            to: foreground,
            ratio: achieved,
        });
    }

    let mut out = css.to_string();
    for (range, text) in edits.into_iter().rev() {
        out.replace_range(range, &text);
    }
    FloorPass {
        css: out,
        lifted,
        unrepaired,
        foreground_ratio: ratio(foreground, ground),
    }
}

/// A declaration block: the selector text before it and the text between braces.
struct Block {
    selector: std::ops::Range<usize>,
    body: std::ops::Range<usize>,
}

/// Every **innermost** `{ … }` block — the ones holding declarations rather than
/// nested rules — paired with the selector text preceding them.
///
/// Scanning for innermost blocks rather than assuming a flat sheet is what lets
/// the pass run over a composed stylesheet: `ikigai-browse` wraps each theme's
/// generated CSS in an `@media (prefers-color-scheme: …)` block, and a pass that
/// only understood top-level rules would silently do nothing there — the failure
/// mode being a clean run that changes nothing.
fn declaration_blocks(css: &str) -> Vec<Block> {
    let bytes = css.as_bytes();
    let mut blocks = Vec::new();
    // Start of the current selector run: after the last brace or comment.
    let mut selector_start = 0usize;
    // Stack of (open brace index, selector range, whether a nested block opened).
    let mut stack: Vec<(usize, std::ops::Range<usize>, bool)> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        // Comments are not structure: syntect writes a banner one, and a stray
        // brace inside a comment must not open a block.
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let end = css[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |off| i + 2 + off + 2);
            i = end;
            selector_start = i;
            continue;
        }
        match bytes[i] {
            b'{' => {
                if let Some(top) = stack.last_mut() {
                    top.2 = true; // the enclosing block has a nested one
                }
                stack.push((i, selector_start..i, false));
                selector_start = i + 1;
            }
            b'}' => {
                if let Some((open, selector, nested)) = stack.pop() {
                    if !nested {
                        blocks.push(Block {
                            selector,
                            body: open + 1..i,
                        });
                    }
                }
                selector_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    blocks
}

/// One declaration's spans within a block body.
struct Decl {
    value: std::ops::Range<usize>,
}

/// Find `property`'s value span in a declaration block body, matching the
/// property name EXACTLY — `color` must not match `background-color`, which is
/// the whole difficulty of doing this with string search.
fn declaration(body: &str, property: &str) -> Option<Decl> {
    let mut offset = 0usize;
    for piece in body.split(';') {
        let start = offset;
        offset += piece.len() + 1; // the ';' the split consumed
        let Some(colon) = piece.find(':') else {
            continue;
        };
        if piece[..colon].trim() != property {
            continue;
        }
        let raw = &piece[colon + 1..];
        let leading = raw.len() - raw.trim_start().len();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value_start = start + colon + 1 + leading;
        return Some(Decl {
            value: value_start..value_start + trimmed.len(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DARK_GROUND: &str = "#2b303b";
    const DARK_FOREGROUND: &str = "#c0c5ce";

    fn dark() -> (Rgba, Rgba) {
        (
            Rgba::parse(DARK_GROUND).unwrap(),
            Rgba::parse(DARK_FOREGROUND).unwrap(),
        )
    }

    /// The pass's contract, both halves at once: a failing rule is lifted to the
    /// theme's own foreground, and a passing rule is not touched.
    #[test]
    fn a_failing_rule_is_lifted_and_a_passing_one_is_untouched() {
        let (ground, foreground) = dark();
        // #bf616a is base16-ocean.dark's red at 3.23:1 — the exact fall-through
        // that made ikigai-browse hand-write DARK_PARAMETER.
        let css = ".hl-variable {\n color: #bf616a;\n}\n.hl-string {\n color: #a3be8c;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1);
        assert_eq!(pass.lifted[0].selector, ".hl-variable");
        assert_eq!(pass.lifted[0].to, foreground);
        assert!((pass.lifted[0].ratio - 3.23).abs() < 0.01);
        assert!(pass.css.contains(".hl-variable {\n color: #c0c5ce;\n}"));
        assert!(
            pass.css.contains(".hl-string {\n color: #a3be8c;\n}"),
            "a passing rule is byte-identical: {}",
            pass.css
        );
        assert!(pass.repairable(4.5));
        assert!((pass.foreground_ratio - 7.63).abs() < 0.01);
    }

    /// Nothing to fix means the input back, unchanged — so a consumer can apply
    /// the pass unconditionally without diffing.
    #[test]
    fn a_sheet_that_already_clears_the_floor_is_returned_verbatim() {
        let (ground, foreground) = dark();
        let css = ".hl-string {\n color: #a3be8c;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.css, css);
        assert!(pass.lifted.is_empty());
    }

    /// `color` must not match the tail of `background-color`. Getting this wrong
    /// repaints backgrounds with the foreground colour — a spectacular failure
    /// that a naive `contains("color:")` walks straight into.
    #[test]
    fn background_color_is_not_mistaken_for_color() {
        let (ground, foreground) = dark();
        // The background is dark and low-contrast against the page ground, but it
        // is a BACKGROUND: it must survive untouched.
        let css = ".hl-code {\n background-color: #2b303b;\n color: #c0c5ce;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.lifted.is_empty(), "{:?}", pass.lifted);
        assert_eq!(pass.css, css);
    }

    /// A rule that repaints its own ground is measured against that ground, not
    /// against the page's — otherwise a legible highlight-on-highlight pair reads
    /// as a failure and gets "repaired" into an actual one.
    #[test]
    fn a_rule_with_its_own_background_is_measured_against_that_background() {
        let (ground, foreground) = dark();
        // White on near-black: 1.1:1 against the PAGE ground (#2b303b), but the
        // rule paints its own black ground, where it is ~19:1.
        let css = ".hl-inverse {\n background-color: #000000;\n color: #ffffff;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.lifted.is_empty(), "{:?}", pass.lifted);
    }

    /// The pass has to reach inside `@media` blocks: that is exactly how
    /// ikigai-browse composes its light and dark sheets, so a pass that only saw
    /// top-level rules would run clean and change nothing.
    #[test]
    fn rules_nested_in_a_media_block_are_reached() {
        let (ground, foreground) = dark();
        let css = "@media (prefers-color-scheme: dark) {\n.hl-variable {\n color: #bf616a;\n}\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1);
        assert_eq!(pass.lifted[0].selector, ".hl-variable");
        assert!(pass.css.contains("color: #c0c5ce"), "{}", pass.css);
        assert!(pass.css.starts_with("@media"));
    }

    /// A brace inside syntect's banner comment must not open a block.
    #[test]
    fn a_comment_is_not_structure() {
        let (ground, foreground) = dark();
        let css =
            "/* theme \"x\" { generated } by syntect */\n.hl-variable {\n color: #bf616a;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1, "{:?}", pass.lifted);
        assert_eq!(pass.lifted[0].selector, ".hl-variable");
    }

    /// Multi-selector and deeply-compound selectors survive as written — syntect
    /// emits selectors of up to 33 classes.
    #[test]
    fn a_compound_selector_is_reported_as_written() {
        let (ground, foreground) = dark();
        let css = ".hl-meta.hl-function .hl-entity.hl-name, .hl-variable.hl-parameter {\n color: #bf616a;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(
            pass.lifted[0].selector,
            ".hl-meta.hl-function .hl-entity.hl-name, .hl-variable.hl-parameter"
        );
    }

    /// An unreadable value is left alone rather than rewritten: this pass fixes
    /// contrast, it does not normalize CSS.
    #[test]
    fn an_unparseable_colour_is_left_alone() {
        let (ground, foreground) = dark();
        let css = ".hl-x {\n color: var(--fg);\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.css, css);
        assert!(pass.lifted.is_empty());
    }

    /// A theme whose own foreground fails the floor cannot be repaired from its
    /// own palette, and the pass says so instead of pretending — leaving the
    /// stylesheet exactly as the theme wrote it.
    #[test]
    fn an_unrepairable_theme_reports_itself_and_changes_nothing() {
        let ground = Rgba::parse("#2b303b").unwrap();
        let washed_out = Rgba::parse("#3a3f4b").unwrap();
        let css = ".hl-x {\n color: #bf616a;\n}\n";
        let pass = apply_floor(css, ground, washed_out, 4.5);
        assert!(!pass.repairable(4.5));
        assert!(pass.foreground_ratio < 4.5);
        assert!(pass.lifted.is_empty(), "no fix is better than a fake one");
        assert_eq!(pass.unrepaired.len(), 1);
        assert_eq!(pass.unrepaired[0].selector, ".hl-x");
        assert!(!pass.clears_floor());
        assert_eq!(pass.css, css);
    }

    /// A rule that repaints a low-contrast ground of its own cannot be repaired
    /// by the theme's foreground either — the failure is the pair, not the text
    /// colour. Reported, not rewritten, which is also what makes the pass
    /// idempotent: run it twice and the second run changes nothing.
    #[test]
    fn a_rule_whose_own_ground_defeats_the_foreground_is_reported_not_rewritten() {
        let (ground, foreground) = dark();
        // #c0c5ce on #6b7280 is ~2.5:1 — the rule's own background is the problem.
        let css = ".hl-meta.hl-separator {\n background-color: #6b7280;\n color: #c0c5ce;\n}\n";
        let first = apply_floor(css, ground, foreground, 4.5);
        assert!(first.lifted.is_empty());
        assert_eq!(first.unrepaired.len(), 1);
        assert_eq!(first.css, css);
        let second = apply_floor(&first.css, ground, foreground, 4.5);
        assert_eq!(second.css, first.css, "idempotent");
    }

    /// A translucent scope colour is composited before it is judged: fully
    /// transparent text is 1:1, not whatever hue it nominally names.
    #[test]
    fn a_translucent_colour_is_composited_before_being_judged() {
        let (ground, foreground) = dark();
        let css = ".hl-ghost {\n color: #ffffff00;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1);
        assert!((pass.lifted[0].ratio - 1.0).abs() < 1e-6);
    }
}
