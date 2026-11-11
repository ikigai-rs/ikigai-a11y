//! The contrast-floor pass: repair a generated stylesheet **in its own palette**.
//!
//! A syntax theme is a palette plus a ground. Some of its scope colors clear the
//! contrast floor against that ground and some do not — and which is which is a
//! property of the theme, not of the code being highlighted, so it can be
//! computed once and repaired once for every theme, rather than patched by hand
//! per scope as it is noticed.
//!
//! The repair color is **the theme's own default foreground**. That is the one
//! color a theme guarantees is legible on its own ground, so lifting to it
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
//! the `themes` face (feature `themes`).

use crate::color::{ratio, Rgba};

/// One rule the pass changed.
#[derive(Clone, PartialEq, Debug)]
pub struct Lift {
    /// The rule's selector, as written.
    pub selector: String,
    /// The color it declared.
    pub from: Rgba,
    /// The color it now declares — always the theme's own foreground.
    pub to: Rgba,
    /// The ratio the original color achieved against its ground.
    pub ratio: f64,
}

/// A rule below the floor that the pass did **not** rewrite, because the
/// theme's own foreground would not clear the floor on that rule's ground
/// either — so there is no in-palette repair, and inventing one is not this
/// pass's business. A rule that paints a ground and sets no color is one of these
/// when the foreground its text inherits does not clear that ground.
#[derive(Clone, PartialEq, Debug)]
pub struct Finding {
    /// The rule's selector, as written.
    pub selector: String,
    /// The color it declares, or the theme's foreground when it declares none.
    pub color: Rgba,
    /// The ratio it achieves against its ground.
    pub ratio: f64,
}

/// A value the pass needed and could not read, left exactly as written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Skip {
    /// The rule's selector, as written.
    pub selector: String,
    /// `color`, or `background-color` or `background` when the rule's own ground
    /// is unreadable.
    pub property: &'static str,
    /// The declared value, as written.
    pub value: String,
}

/// The result of a floor pass: the rewritten stylesheet and what it changed.
#[derive(Clone, PartialEq, Debug)]
pub struct FloorPass {
    /// The stylesheet with sub-floor foreground colors lifted.
    pub css: String,
    /// Every rule that was lifted, in source order.
    pub lifted: Vec<Lift>,
    /// Every rule below the floor that could not be repaired in-palette — left
    /// exactly as the theme wrote it, and reported rather than hidden. A caller
    /// that wants a guarantee rather than a best effort checks this is empty.
    pub unrepaired: Vec<Finding>,
    /// The repair color's OWN ratio against the page ground.
    ///
    /// Normally high — a theme's foreground is legible on its own ground by
    /// construction — but a theme where it is below the floor cannot be repaired
    /// from its own palette at all, and the honest answer for such a theme is to
    /// not offer it at that floor rather than to invent a color for it.
    pub foreground_ratio: f64,
}

impl FloorPass {
    /// Whether the repair color itself clears `min` — i.e. whether the pass
    /// could do its job at all on this ground.
    pub fn repairable(&self, min: f64) -> bool {
        self.foreground_ratio >= min
    }

    /// Whether every rule now clears the floor: the pass repaired what it found,
    /// left nothing behind, and read every value it needed. A value it could not
    /// read (see [`skipped`](Self::skipped)) is not proved to clear anything.
    pub fn clears_floor(&self) -> bool {
        self.unrepaired.is_empty() && self.skipped().is_empty()
    }

    /// Every value the pass needed and could not read, rule by rule in source
    /// order (within a rule, `color` before its ground): the `color` of a rule
    /// written as a named color, `hsl()`, `var()`, a value carrying `!important`, or
    /// anything else [`Rgba::parse`] refuses, and the `background-color` or
    /// `background` of a rule that declares a `color` when that ground is unreadable,
    /// as an image is (the rule is then judged against the page ground instead).
    ///
    /// The pass leaves each of these exactly as written, so they are found in
    /// [`css`](Self::css). None of them was measured, which is why a re-run of the
    /// pass that lifts nothing proves nothing about them.
    pub fn skipped(&self) -> Vec<Skip> {
        unread(&self.css)
    }
}

/// Lift every `color:` declaration whose contrast against its ground is below
/// `min` to `foreground`, leaving everything else — including every declaration
/// that already clears the floor — byte-for-byte unchanged.
///
/// A rule that paints its own ground, with `background-color` or the `background`
/// shorthand, is measured against **that**, not against the page ground: a rule
/// which repaints its own ground has changed what its text sits on. If it sets no
/// color its text inherits one, so the theme's foreground is measured there and a
/// failure is reported in [`FloorPass::unrepaired`]. Translucent colors are
/// composited before measuring (see [`crate::color`]).
///
/// A value the pass cannot read (a named color, `hsl()`, `var()`, `!important`)
/// is left as written and listed by [`FloorPass::skipped`], never dropped.
pub fn apply_floor(css: &str, ground: Rgba, foreground: Rgba, min: f64) -> FloorPass {
    let replacement = foreground.to_css();
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut lifted = Vec::new();
    let mut unrepaired = Vec::new();

    for block in declaration_blocks(css) {
        let body = &css[block.body.clone()];
        // The rule's own ground, if it repaints one.
        let painted = declaration(body, GROUND)
            .and_then(|d| painted_ground(d.property, &body[d.value]).ok())
            .flatten();
        let local_ground = painted.map_or(ground, |bg| bg.over(ground));
        let Some(color) = declaration(body, &["color"]) else {
            // Its text inherits a color, and the one this pass knows is the
            // theme's foreground. There is no declaration to lift: report the pair.
            let achieved = ratio(foreground, local_ground);
            if painted.is_some() && achieved < min {
                unrepaired.push(Finding {
                    selector: css[block.selector.clone()].trim().to_string(),
                    color: foreground,
                    ratio: achieved,
                });
            }
            continue;
        };
        let Ok(current) = Rgba::parse(&body[color.value.clone()]) else {
            // An unparseable color is left alone: this pass repairs contrast, it
            // does not police syntax, and a value it cannot read is one it cannot
            // reason about either. It is not dropped silently: `skipped` lists it.
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

/// The values [`apply_floor`] needs and cannot read, by the same reading it does:
/// each rule's `color`, and the ground of a rule that has a `color`.
fn unread(css: &str) -> Vec<Skip> {
    let mut skipped = Vec::new();
    for block in declaration_blocks(css) {
        let body = &css[block.body.clone()];
        let Some(color) = declaration(body, &["color"]) else {
            continue;
        };
        let selector = css[block.selector.clone()].trim();
        let mut skip = |property, value: &str| {
            skipped.push(Skip {
                selector: selector.to_string(),
                property,
                value: value.to_string(),
            });
        };
        let value = &body[color.value];
        if Rgba::parse(value).is_err() {
            skip(color.property, value);
        }
        if let Some(own) = declaration(body, GROUND) {
            let value = &body[own.value];
            if painted_ground(own.property, value).is_err() {
                skip(own.property, value);
            }
        }
    }
    skipped
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
    let mut blocks = Vec::new();
    // Start of the current selector run: after the last structural brace.
    let mut selector_start = 0usize;
    // Stack of (open brace index, selector range, whether a nested block opened).
    let mut stack: Vec<(usize, std::ops::Range<usize>, bool)> = Vec::new();
    let mut depth = 0usize;
    for (i, byte) in syntax(css) {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'{' if depth == 0 => {
                if let Some(top) = stack.last_mut() {
                    top.2 = true;
                }
                let start = skip_trivia(css, selector_start);
                stack.push((i, start..i, false));
                selector_start = i + 1;
            }
            b'}' if depth == 0 => {
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
    }
    blocks
}

/// Bytes outside CSS comments, quoted strings, and escapes. Only these bytes
/// can delimit blocks or declarations. Offsets always refer to the original
/// input so repairs preserve every byte outside the measured value.
fn syntax(text: &str) -> impl Iterator<Item = (usize, u8)> + '_ {
    let bytes = text.as_bytes();
    let mut i = 0usize;
    std::iter::from_fn(move || {
        while i < bytes.len() {
            let at = i;
            let byte = bytes[i];
            i += 1;
            match byte {
                b'/' if bytes.get(i) == Some(&b'*') => {
                    i = comment_end(text, at);
                }
                b'\\' => i = (i + 1).min(bytes.len()),
                b'\'' | b'"' => {
                    while i < bytes.len() {
                        let next = bytes[i];
                        i += 1;
                        if next == b'\\' {
                            i = (i + 1).min(bytes.len());
                        } else if next == byte {
                            break;
                        }
                    }
                }
                _ => return Some((at, byte)),
            }
        }
        None
    })
}

fn comment_end(text: &str, start: usize) -> usize {
    text[start + 2..]
        .find("*/")
        .map_or(text.len(), |off| start + 2 + off + 2)
}

/// Skip whitespace and comments between CSS tokens without changing the input.
fn skip_trivia(text: &str, mut i: usize) -> usize {
    let bytes = text.as_bytes();
    loop {
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i..i + 2) == Some(b"/*") {
            i = comment_end(text, i);
        } else {
            return i;
        }
    }
}

/// One declaration within a block body: which property, and its value's span.
struct Decl {
    property: &'static str,
    value: std::ops::Range<usize>,
}

/// The two properties that set a rule's own ground. Each overrides the other, so
/// whichever applies is the one that decides it.
const GROUND: &[&str] = &["background-color", "background"];

/// Find the value span of the declaration CSS applies among `properties` in a
/// declaration block body, matching the property name EXACTLY — `color` must not
/// match `background-color`. Names are ASCII case-insensitive, as in CSS, and
/// comments between tokens do not hide declarations. This cannot use simple
/// string search.
///
/// The declaration that applies is the cascade's within one block (ledger
/// #2908): the LAST `!important` one if there is any, otherwise the LAST one. An
/// earlier declaration is overridden and never renders, so measuring it would
/// judge a color nobody sees and lift one that changes nothing.
///
/// Not modeled: a browser also drops a declaration whose value it cannot parse
/// and falls back to the one before, and this reader cannot tell a value the
/// browser refuses from one it accepts and [`Rgba::parse`] does not (a named
/// color). Either way the applied value is listed by [`FloorPass::skipped`] when
/// it does not parse, so nothing it is unsure of reads as clearing the floor.
fn declaration(body: &str, properties: &[&'static str]) -> Option<Decl> {
    let mut last = None;
    let mut last_important = None;
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut ends = Vec::new();
    for (i, byte) in syntax(body) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => ends.push(i),
            _ => {}
        }
    }
    ends.push(body.len());
    for end in ends {
        let piece_start = start;
        start = end + 1;
        let piece = &body[piece_start..end];
        let Some((colon, _)) = syntax(piece).find(|(_, byte)| *byte == b':') else {
            continue;
        };
        // Comments may precede the name or separate it from the colon, but
        // cannot join two identifier tokens (col/**/or is not color).
        let name_start = skip_trivia(piece, 0);
        let name_end = piece[name_start..colon]
            .find(|c: char| c.is_ascii_whitespace() || c == '/')
            .map_or(colon, |off| name_start + off);
        if skip_trivia(piece, name_end) != colon {
            continue;
        }
        let name = &piece[name_start..name_end];
        let Some(property) = properties
            .iter()
            .copied()
            .find(|p| p.eq_ignore_ascii_case(name))
        else {
            continue;
        };
        let raw = &piece[colon + 1..];
        let leading = raw.len() - raw.trim_start().len();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value_start = piece_start + colon + 1 + leading;
        let decl = Decl {
            property,
            value: value_start..value_start + trimmed.len(),
        };
        if is_important(trimmed) {
            last_important = Some(decl);
        } else {
            last = Some(decl);
        }
    }
    last_important.or(last)
}

/// The color a ground declaration paints: `Ok(None)` when it paints none, `Err`
/// when the pass cannot read what it paints.
fn painted_ground(property: &str, value: &str) -> Result<Option<Rgba>, ()> {
    if property == "background" {
        return shorthand_color(value);
    }
    Rgba::parse(value).map(Some).map_err(drop)
}

/// The color component of a `background` shorthand, which CSS allows in any
/// position. Only a single layer with no image is read: an image, a gradient or
/// a second layer has no one color to measure against, and `!important` or a
/// color the parser refuses is unread here just as it is in `background-color`.
/// With no image, a position, size, repeat, attachment or box paints nothing.
fn shorthand_color(value: &str) -> Result<Option<Rgba>, ()> {
    let mut color = None;
    for part in shorthand_parts(value) {
        if paints_nothing(part) {
            continue;
        }
        let parsed = Rgba::parse(part).map_err(drop)?;
        if color.replace(parsed).is_some() {
            return Err(());
        }
    }
    Ok(color)
}

/// A shorthand's parts, split at spaces and `/` outside parentheses.
fn shorthand_parts(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in value.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '/' | ' ' | '\t' | '\n' | '\r' | '\x0c' if depth == 0 => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts.retain(|part| !part.is_empty());
    parts
}

/// Whether a shorthand part leaves the color unaffected and adds no image: a
/// number, length or percentage (position, size), or one of the keywords.
fn paints_nothing(part: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "none",
        "repeat",
        "repeat-x",
        "repeat-y",
        "no-repeat",
        "space",
        "round",
        "scroll",
        "fixed",
        "local",
        "border-box",
        "padding-box",
        "content-box",
        "center",
        "top",
        "bottom",
        "left",
        "right",
        "auto",
        "cover",
        "contain",
    ];
    // The sign is stripped first: `-webkit-linear-gradient(…)` is an image.
    part.trim_start_matches(['+', '-'])
        .starts_with(|c: char| c.is_ascii_digit() || c == '.')
        || KEYWORDS.iter().any(|k| part.eq_ignore_ascii_case(k))
}

/// Whether a declaration's value carries the `!important` flag (`!` and the
/// ASCII case-insensitive keyword, whitespace allowed between them).
fn is_important(value: &str) -> bool {
    let Some((bang, _)) = syntax(value).filter(|(_, byte)| *byte == b'!').last() else {
        return false;
    };
    let keyword = skip_trivia(value, bang + 1);
    let end = keyword + "important".len();
    value
        .get(keyword..end)
        .is_some_and(|word| word.eq_ignore_ascii_case("important"))
        && skip_trivia(value, end) == value.len()
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

    #[test]
    fn property_names_are_ascii_case_insensitive() {
        let (ground, foreground) = dark();
        let css = ".x { COLOR: #bf616a; }";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.css, ".x { COLOR: #c0c5ce; }");
        assert_eq!(pass.lifted.len(), 1);
        assert_eq!(
            apply_floor(&pass.css, ground, foreground, 4.5).css,
            pass.css
        );

        for property in ["BACKGROUND", "Background-Color"] {
            let css = format!(".x {{ {property}: #eeeeee; color: #ffffff; }}");
            let pass = apply_floor(&css, ground, foreground, 4.5);
            assert_eq!(pass.unrepaired.len(), 1, "{pass:?}");
            assert!(!pass.clears_floor());
        }
    }

    #[test]
    fn comments_before_properties_do_not_hide_declarations() {
        let (ground, foreground) = dark();
        for css in [
            ".x { /* note */ color: #bf616a; }",
            ".x { color /* note */ : #bf616a; }",
            ".x { /* misleading ; color: #ffffff; */ color: #bf616a; }",
        ] {
            let pass = apply_floor(css, ground, foreground, 4.5);
            assert_eq!(pass.lifted.len(), 1, "{css}: {pass:?}");
            assert_eq!(pass.css, css.replace("#bf616a", "#c0c5ce"));
            assert_eq!(
                apply_floor(&pass.css, ground, foreground, 4.5).css,
                pass.css
            );
        }
        let css = ".x { /* note */ background: #eeeeee; color: #ffffff; }";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.unrepaired.len(), 1, "{pass:?}");
        assert!(!pass.clears_floor());
    }

    #[test]
    fn strings_and_function_contents_are_not_declaration_structure() {
        let (ground, foreground) = dark();
        for css in [
            r#".x { content: "}"; color: #bf616a; }"#,
            r#".x { content: "; color: #ffffff;"; color: #bf616a; }"#,
            r#".x { content: "/*"; color: #bf616a; }"#,
            r#".x[data-label="{"] { color: #bf616a; }"#,
            ".x { --data: fn(; color: #ffffff;); color: #bf616a; }",
        ] {
            let pass = apply_floor(css, ground, foreground, 4.5);
            assert_eq!(pass.lifted.len(), 1, "{css}: {pass:?}");
            assert_eq!(pass.css, css.replace("#bf616a", "#c0c5ce"));
            assert_eq!(
                apply_floor(&pass.css, ground, foreground, 4.5).css,
                pass.css
            );
        }
        let css = r#".x { color: #bf616a; content: "; color: #ffffff;"; }"#;
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1, "{pass:?}");
        assert_eq!(pass.css, css.replace("#bf616a", "#c0c5ce"));
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
    /// repaints backgrounds with the foreground color — a spectacular failure
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
    fn an_unparseable_color_is_left_alone() {
        let (ground, foreground) = dark();
        let css = ".hl-x {\n color: var(--fg);\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.css, css);
        assert!(pass.lifted.is_empty());
        assert_eq!(pass.skipped().len(), 1, "left alone, but listed");
    }

    /// Every value the pass cannot read is LISTED, never silently dropped (ledger
    /// #1652, point 3): a named color, `hsl()`, `var()` and a value carrying
    /// `!important` are left as written, and so is a rule whose own ground cannot
    /// be read. Each is outside what the pass measured, so the sheet is not proved
    /// to clear the floor.
    #[test]
    fn every_value_the_pass_cannot_read_is_listed() {
        let (ground, foreground) = dark();
        let css = ".a {\n color: red;\n}\n\
                   .b {\n color: hsl(0 0% 50%);\n}\n\
                   .c {\n color: var(--fg);\n}\n\
                   .d {\n color: #bf616a !important;\n}\n\
                   .e {\n background-color: var(--bg);\n color: #c0c5ce;\n}\n\
                   .f {\n color: #a3be8c;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.css, css, "nothing it cannot read is rewritten");
        let skips = pass.skipped();
        let skipped: Vec<(&str, &str, &str)> = skips
            .iter()
            .map(|s| (s.selector.as_str(), s.property, s.value.as_str()))
            .collect();
        assert_eq!(
            skipped,
            [
                (".a", "color", "red"),
                (".b", "color", "hsl(0 0% 50%)"),
                (".c", "color", "var(--fg)"),
                (".d", "color", "#bf616a !important"),
                (".e", "background-color", "var(--bg)"),
            ]
        );
        assert!(pass.unrepaired.is_empty());
        assert!(
            !pass.clears_floor(),
            "a value the pass could not read is not proved to clear the floor"
        );
    }

    /// A sheet the pass reads in full lists nothing.
    #[test]
    fn a_sheet_read_in_full_lists_nothing_skipped() {
        let (ground, foreground) = dark();
        let css = ".x {\n background-color: #000000;\n color: #ffffff;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.skipped().is_empty());
        assert!(pass.clears_floor());
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
    /// color. Reported, not rewritten, which is also what makes the pass
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

    /// A translucent scope color is composited before it is judged: fully
    /// transparent text is 1:1, not whatever hue it nominally names.
    #[test]
    fn a_translucent_color_is_composited_before_being_judged() {
        let (ground, foreground) = dark();
        let css = ".hl-ghost {\n color: #ffffff00;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1);
        assert!((pass.lifted[0].ratio - 1.0).abs() < 1e-6);
    }

    /// When a block declares `color` twice, CSS uses the LAST declaration (ledger
    /// #2908), so the last is the one measured and the one lifted, and the
    /// overridden one is left exactly as written.
    #[test]
    fn the_last_color_declaration_is_the_one_measured_and_lifted() {
        let (ground, foreground) = dark();
        // The rendered color is #bf616a (3.23:1); the overridden #c0c5ce passes.
        let css = ".hl-x {\n color: #c0c5ce;\n color: #bf616a;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1, "{pass:?}");
        assert_eq!(pass.lifted[0].from, Rgba::parse("#bf616a").unwrap());
        assert_eq!(
            pass.css, ".hl-x {\n color: #c0c5ce;\n color: #c0c5ce;\n}\n",
            "the declaration that renders is the one rewritten"
        );

        // And the converse: an overridden failing color is not what renders, so a
        // block whose last color passes is left byte-for-byte alone.
        let css = ".hl-y {\n color: #bf616a;\n color: #a3be8c;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.lifted.is_empty(), "{:?}", pass.lifted);
        assert_eq!(pass.css, css);
        assert!(pass.clears_floor());
    }

    /// The rule's own ground is its LAST `background-color` too.
    #[test]
    fn the_last_background_color_is_the_ground_measured_against() {
        let (ground, foreground) = dark();
        // On #000000 the foreground is legible; on #6b7280, the ground that
        // renders, it is ~2.5:1 and the pair cannot be repaired in-palette.
        let css = ".hl-z {\n background-color: #000000;\n background-color: #6b7280;\n color: #c0c5ce;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.unrepaired.len(), 1, "{pass:?}");
        assert_eq!(pass.css, css);
    }

    /// An `!important` declaration wins over a later normal one, as in CSS, so
    /// the value that renders is the important one: unread, and listed.
    #[test]
    fn an_important_declaration_wins_over_a_later_normal_one() {
        let (ground, foreground) = dark();
        let css = ".hl-w {\n color: #bf616a !important;\n color: #a3be8c;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.lifted.is_empty(), "{:?}", pass.lifted);
        assert_eq!(pass.css, css);
        let skips = pass.skipped();
        assert_eq!(skips.len(), 1, "{skips:?}");
        assert_eq!(skips[0].value, "#bf616a !important");
        assert!(!pass.clears_floor());

        // Of two important declarations, the last wins; `! important` with a space
        // is the same flag.
        let css = ".hl-v {\n color: #a3be8c ! IMPORTANT;\n color: #bf616a !important;\n color: #c0c5ce;\n}\n";
        let skips = apply_floor(css, ground, foreground, 4.5).skipped();
        assert_eq!(skips.len(), 1, "{skips:?}");
        assert_eq!(skips[0].value, "#bf616a !important");
    }

    #[test]
    fn comments_in_important_flags_do_not_change_the_cascade() {
        let (ground, foreground) = dark();
        for flag in [
            "!/**/important",
            "! important /* note */",
            "!/* ; */IMPORTANT",
        ] {
            let css = format!(".x {{ color: #bf616a {flag}; color: #c0c5ce; }}");
            let pass = apply_floor(&css, ground, foreground, 4.5);
            assert_eq!(pass.css, css);
            assert_eq!(pass.skipped().len(), 1, "{flag}: {pass:?}");
            assert_eq!(pass.skipped()[0].value, format!("#bf616a {flag}"));
            assert!(!pass.clears_floor());
        }
    }

    /// A `background` shorthand paints the rule's ground exactly as a
    /// `background-color` does (ledger #1652, point 4). White text passes on the
    /// dark page ground, so reading only `background-color` calls this rule clean
    /// while its text renders at 1.16:1 on the color it paints.
    #[test]
    fn a_background_shorthand_is_the_ground_measured_against() {
        let (ground, foreground) = dark();
        let css = ".hl-x {\n background: #eeeeee;\n color: #ffffff;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.unrepaired.len(), 1, "{pass:?}");
        assert_eq!(pass.css, css);

        // The converse: on its own black ground this red passes (it is 3.23:1 on
        // the page), so a shorthand ground saves it from a false lift. The color
        // is found among the shorthand's other parts.
        let css = ".hl-y {\n background: no-repeat center / cover #000000;\n color: #bf616a;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.lifted.is_empty(), "{:?}", pass.lifted);
        assert!(pass.clears_floor(), "{pass:?}");
    }

    /// Of `background` and `background-color`, the later one applies, because the
    /// shorthand resets the color: `background: none` takes a ground away.
    #[test]
    fn the_later_of_the_shorthand_and_the_longhand_is_the_ground() {
        let (ground, foreground) = dark();
        for css in [
            ".x {\n background: #000000;\n background-color: #eeeeee;\n color: #ffffff;\n}\n",
            ".x {\n background-color: #000000;\n background: #eeeeee;\n color: #ffffff;\n}\n",
        ] {
            let pass = apply_floor(css, ground, foreground, 4.5);
            assert_eq!(pass.unrepaired.len(), 1, "{css}: {pass:?}");
        }
        // No paint: the white text sits on the dark page ground, where it passes.
        let css = ".x {\n background-color: #eeeeee;\n background: none;\n color: #ffffff;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert!(pass.clears_floor(), "{pass:?}");
    }

    /// A shorthand whose ground cannot be read is listed, not guessed at: an image
    /// has no one color to measure, whatever color is painted beneath it.
    #[test]
    fn a_shorthand_the_pass_cannot_read_is_listed() {
        let (ground, foreground) = dark();
        for value in [
            "url(tile.png)",
            "url(tile.png) #000000",
            "linear-gradient(#000000, #ffffff)",
            "-webkit-linear-gradient(#000000, #ffffff)",
            "url(a.png), #000000",
            "red",
            "var(--bg)",
            "#000000 !important",
            "#000000 #111111",
        ] {
            let css = format!(".x {{\n background: {value};\n color: #ffffff;\n}}\n");
            let pass = apply_floor(&css, ground, foreground, 4.5);
            assert_eq!(pass.css, css);
            let skips = pass.skipped();
            assert_eq!(skips.len(), 1, "{value}: {skips:?}");
            assert_eq!(skips[0].property, "background");
            assert_eq!(skips[0].value, value);
            assert!(!pass.clears_floor(), "{value}");
        }
    }

    /// A rule that paints a ground and sets no color has its text inherit one, so
    /// the pair is judged with the theme's foreground. SolarizedDark's `.hl-invalid`
    /// is this rule, at 3.16:1, and the pass used to call that theme fully repaired.
    #[test]
    fn a_ground_painted_without_a_color_is_judged_with_the_foreground() {
        let ground = Rgba::parse("#002b36").unwrap();
        let foreground = Rgba::parse("#839496").unwrap();
        for css in [
            ".hl-invalid {\n background-color: #6e2e32;\n}\n",
            ".hl-invalid {\n background: #6e2e32;\n}\n",
        ] {
            let pass = apply_floor(css, ground, foreground, 4.5);
            assert_eq!(pass.css, css, "no color to lift, and none is invented");
            assert_eq!(pass.unrepaired.len(), 1, "{pass:?}");
            assert_eq!(pass.unrepaired[0].selector, ".hl-invalid");
            assert_eq!(pass.unrepaired[0].color, foreground);
            assert!((pass.unrepaired[0].ratio - 3.16).abs() < 0.01);
            assert!(!pass.clears_floor());
        }

        // A ground the foreground clears is not a finding.
        let css = ".hl-ok {\n background-color: #000000;\n}\n";
        assert!(apply_floor(css, ground, foreground, 4.5).clears_floor());
    }

    /// Stated limits, pinned so that closing one has to change this test and
    /// ACCESSIBILITY.md together (ledger #1652, points 5 and 6): only `color` is
    /// measured, against the one floor the caller gave, whatever the text's size.
    #[test]
    fn non_text_colors_and_text_size_are_not_considered() {
        let (ground, foreground) = dark();
        // A border the ground's own color is invisible (SC 1.4.11), and not seen.
        let css =
            ".hl-x {\n border-color: #2b303b;\n outline-color: #2b303b;\n color: #c0c5ce;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.css, css);
        assert!(pass.clears_floor());

        // Bold 24px text at 3.23:1 clears WCAG's 3:1 large-text floor, and is lifted
        // all the same. The stricter reading, so it never lets a failure through.
        let css = ".hl-y {\n font-size: 24px;\n font-weight: bold;\n color: #bf616a;\n}\n";
        let pass = apply_floor(css, ground, foreground, 4.5);
        assert_eq!(pass.lifted.len(), 1, "{pass:?}");
    }

    /// A tint written as `rgba(…)` is measured like its hex spelling (ledger
    /// #2904): ikigai-browse once had to rewrite its tints as `#rrggbbaa`
    /// because the pass could not read them, and an unreadable value is
    /// silently skipped.
    #[test]
    fn a_functional_rgb_color_is_measured_like_its_hex_spelling() {
        let (ground, foreground) = dark();
        let functional = apply_floor(
            ".hl-tint {\n color: rgba(255, 255, 255, 0.1);\n}\n",
            ground,
            foreground,
            4.5,
        );
        let hex = apply_floor(
            ".hl-tint {\n color: #ffffff1a;\n}\n",
            ground,
            foreground,
            4.5,
        );
        assert_eq!(functional.lifted.len(), 1, "{functional:?}");
        assert!((functional.lifted[0].ratio - hex.lifted[0].ratio).abs() < 1e-9);
        assert_eq!(
            functional.css, hex.css,
            "both lift to the same hex foreground"
        );

        // And a rule's own ground written that way is the ground it is judged on.
        let grounded = apply_floor(
            ".hl-x {\n background-color: rgb(192 197 206);\n color: #c0c5ce;\n}\n",
            ground,
            foreground,
            4.5,
        );
        assert_eq!(grounded.unrepaired.len(), 1, "{grounded:?}");
    }
}
