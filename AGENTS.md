# Working in this repository

`ikigai-a11y` is an ikigai module: the layered accessibility config (`a11y.toml` and
`{app}.a11y.toml`), WCAG contrast math, and a contrast-floor pass over generated CSS, served as
`urn:a11y:*` resources. These rules are for every agent and every person working here.
`CLAUDE.md` is a link to this file.

- **Read [ACCESSIBILITY.md](ACCESSIBILITY.md) before any change to the contrast math, the floor
  pass, the config schema or anything a host renders from them.** It says what this crate
  guarantees, what it does not check, and what has been tested.
- Security reports go through the organization's security policy (GitHub private vulnerability
  reporting), never a public issue.
- Every change lands through a pull request with green CI: `cargo fmt`, `cargo clippy
  --all-targets --all-features -- -D warnings` and `cargo test --all-features`, plus the wasm
  build of the library (`.github/workflows/ci.yml`).
- Never invent testing, tool results or conformance evidence, and never weaken a test to make a
  change pass.
- Use American spelling in everything you write: code comments, docs, commit messages and pull
  requests.
