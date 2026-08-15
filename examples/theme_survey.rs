//! How safe is it to let an operator pick any embedded theme?
//!
//!     cargo run --features themes --example theme_survey [floor]
//!
//! For every embedded theme, this generates the stylesheet `ikigai-browse` would
//! serve, measures every scope colour against the theme's own ground, and reports
//! how many rules fall below the floor — plus whether the theme's own default
//! foreground clears it, which is what decides whether the repair is possible
//! in-palette at all.
//!
//! The answer is the number that says whether offering free theme choice is safe.

use ikigai_a11y::config::THEMES;
use ikigai_a11y::themes::{identifier, theme_css, ALL};

fn main() {
    let floor: f64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(4.5);

    println!("floor = {floor}:1  ({} embedded themes)\n", ALL.len());
    println!(
        "{:<24} {:>6} {:>7} {:>8} {:>9}  worst rule",
        "theme", "rules", "lifted", "left", "own fg"
    );

    let mut clean = 0usize;
    let mut repaired = 0usize;
    let mut incomplete = Vec::new();
    for theme in ALL {
        let pass = theme_css(*theme, "hl-", floor);
        let rules = pass.css.matches('{').count();
        let worst = pass
            .lifted
            .iter()
            .map(|l| (l.ratio, &l.selector))
            .chain(pass.unrepaired.iter().map(|f| (f.ratio, &f.selector)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if pass.lifted.is_empty() && pass.clears_floor() {
            clean += 1;
        } else if pass.clears_floor() {
            repaired += 1;
        } else {
            incomplete.push(identifier(*theme));
        }
        println!(
            "{:<24} {:>6} {:>7} {:>8} {:>8.2}{}  {}",
            identifier(*theme),
            rules,
            pass.lifted.len(),
            pass.unrepaired.len(),
            pass.foreground_ratio,
            if pass.repairable(floor) { " " } else { "!" },
            worst.map_or_else(
                || "—".to_string(),
                |(ratio, selector)| format!("{ratio:.2}  {selector}")
            )
        );
    }

    println!(
        "\n{clean}/{n} themes clear {floor}:1 unaided; {repaired} more are fully repaired \
         in-palette by the pass;\n{} cannot be{}",
        incomplete.len(),
        if incomplete.is_empty() {
            String::new()
        } else {
            format!(" ({})", incomplete.join(", "))
        },
        n = ALL.len()
    );
    assert_eq!(ALL.len(), THEMES.len());
}
