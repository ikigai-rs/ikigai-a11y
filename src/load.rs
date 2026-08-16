//! The native loading seam: config-home paths in, effective config out.
//!
//! This is the only module that touches a filesystem, and it is `cfg`-gated off
//! wasm — everything else in the crate stays pure so the browser host can link
//! the contrast maths and the merge without a filesystem shim.
//!
//! `std::fs` here rather than a kernel `urn:file:` sub-request is deliberate.
//! The config home is an absolute path outside any host's fs jail, so reading it
//! through `ikigai-fs` would mean either mounting the user's config directory as
//! a second fs root (widening what every `urn:file:` in the host can reach) or
//! the crate silently not working on hosts that hadn't. The golden thread is what
//! the kernel actually needs from the read, and that is declared explicitly —
//! see [`threads`].
//!
//! ## ★ What `urn:cap:a11y:read` does and does not protect
//!
//! It protects the **resource**, not the files. A capability is checked when the
//! kernel resolves an IRI; nothing checks one when a linked library calls a Rust
//! function, and nothing could — a consumer that wanted the bytes could read the
//! TOML with `std::fs` whether or not this module existed. On disk the config is
//! protected by filesystem permissions, and by nothing else.
//!
//! That is not a hole to be plugged, it is the shape of the thing: the gate earns
//! its keep exactly where a gate is the only fence — an agent's manifold, a peer
//! across a transport, an MCP projection. In-process, the host is already reading
//! this person's home directory.
//!
//! So the rule this module enforces is **least privilege instead**:
//!
//! - Deriving an artifact (a stylesheet, a palette, a page)? Call
//!   [`presentation`]. It cannot return the person-facts, so the read that is
//!   not gated cannot reach them.
//! - Serving `urn:a11y:config`, or a process configuring itself for its own
//!   user? Call [`complete`], and know that you are holding assistive-technology
//!   information about a human being.
//!
//! `ikigai-browse` 0.2.12 is why this is written down: it needs two theme names
//! and a floor, and `load` handed it `motion.reduce` and `text.scale` as well —
//! not because it wanted them, but because there was one function. Three more
//! front ends were queued to copy that call.

use std::path::{Path, PathBuf};

use ikigai_core::config::{config_home, layered_paths_in};

use crate::config::{file_iri, A11y, ConfigError, Patch, Presentation};

/// The config file stem every ikigai front end layers.
///
/// Re-exported from [`crate::config`], where it lives so the pure half can name
/// it too: the Turtle face decides a layer's ROLE by comparing a file name to
/// this stem, and that half must build for wasm, where this module does not
/// exist at all.
pub use crate::config::STEM;

/// The candidate config files for `app`, lowest precedence first.
///
/// Both entries are returned whether or not they exist — an absent file is a
/// layer that states nothing, not an error.
pub fn paths(app: Option<&str>) -> Result<Vec<PathBuf>, ConfigError> {
    let home = config_home().ok_or(ConfigError::NoConfigHome)?;
    Ok(layered_paths_in(&home, STEM, app))
}

/// The golden threads the effective config depends on: one per **candidate**
/// path, existing or not.
///
/// Including the paths that do not exist yet is the whole point. A cached config
/// that only declared the files it actually read would not notice an operator
/// CREATING `cms-web.a11y.toml` — the new override would be invisible until the
/// process restarted, which is the failure the golden thread exists to prevent.
///
/// The thread name is the file's `urn:file:` IRI with its **absolute** path, per
/// the convention that a thread is named after the state it tracks. Note that
/// `ikigai-fs` names its threads with paths RELATIVE to a jail root, so these two
/// namespaces never collide (an absolute path starts with `/`) — but a host that
/// wants edits to a11y.toml to invalidate must watch the config home and cut
/// these names; the fs module's own watcher, rooted at the working directory,
/// will not see them.
pub fn threads(app: Option<&str>) -> Result<Vec<String>, ConfigError> {
    Ok(paths(app)?.iter().map(|p| file_iri(p)).collect())
}

/// The **rendering half** of the effective config for `app` — themes, contrast
/// floors, link underlining — from the ambient config home.
///
/// This is the read a consumer deriving an artifact wants, and the one to reach
/// for by default. It states nothing about the person at the keyboard, so a
/// library call that is not capability-checked (and cannot be — see the module
/// docs) cannot reach `motion.reduce` or `text.scale` through it.
///
/// Same files, same layering, same [`threads`] as [`complete`]: only the view
/// differs.
pub fn presentation(app: Option<&str>) -> Result<Presentation, ConfigError> {
    complete(app).map(|c| c.presentation())
}

/// [`presentation`] rooted at an explicit config home — the testable form.
pub fn presentation_in(home: &Path, app: Option<&str>) -> Result<Presentation, ConfigError> {
    complete_in(home, app).map(|c| c.presentation())
}

/// The **whole** effective config for `app`, person-facts included, from the
/// ambient config home.
///
/// Whether this human needs reduced motion and larger text is
/// assistive-technology information about them. Two callers are entitled to it:
/// whatever serves `urn:a11y:config` (which checks [`crate::CAP_READ`] first),
/// and a process configuring itself for the user it is running as. Anything
/// deriving an artifact wants [`presentation`] instead.
pub fn complete(app: Option<&str>) -> Result<A11y, ConfigError> {
    let home = config_home().ok_or(ConfigError::NoConfigHome)?;
    complete_in(&home, app)
}

/// The effective config for `app` from the ambient config home.
#[deprecated(
    since = "0.2.0",
    note = "an artifact-deriving consumer wants `presentation`, which cannot return the \
            person-facts; `complete` is the same read under a name that says what it hands over"
)]
pub fn load(app: Option<&str>) -> Result<A11y, ConfigError> {
    complete(app)
}

/// [`load`] rooted at an explicit config home.
#[deprecated(
    since = "0.2.0",
    note = "use `presentation_in` for the rendering half, or `complete_in` for the whole config"
)]
pub fn load_in(home: &Path, app: Option<&str>) -> Result<A11y, ConfigError> {
    complete_in(home, app)
}

/// [`complete`] rooted at an explicit config home — the testable form, and the
/// one a host serving someone else's config home wants.
pub fn complete_in(home: &Path, app: Option<&str>) -> Result<A11y, ConfigError> {
    let mut effective = A11y::default();
    for path in layered_paths_in(home, STEM, app) {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            // Absent is the normal case and means "this layer states nothing".
            // Anything else — a permissions problem, a directory where a file
            // should be — is a real failure and must not read as "no config".
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(ConfigError::Unreadable {
                    path,
                    message: e.to_string(),
                })
            }
        };
        let patch = Patch::parse(&text, Some(path.clone()))?;
        effective.apply(&patch);
        effective.layers.push(path);
    }
    Ok(effective)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DEFAULT_DARK, DEFAULT_LIGHT};

    /// A scratch config home that removes itself. No dev-dependency for two
    /// directories.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ikigai-a11y-{}-{}-{tag}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock is after the epoch")
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Scratch(dir)
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join(name), contents).expect("scratch write");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_files_at_all_is_the_defaults_not_an_error() {
        let home = Scratch::new("empty");
        let effective = complete_in(&home.0, Some("cms-web")).unwrap();
        assert_eq!(effective, A11y::default());
        assert!(effective.layers.is_empty());
    }

    /// The end-to-end shape of the layering, on real files: the shared floor
    /// survives an app override that never mentions it.
    #[test]
    fn the_app_file_overrides_the_shared_one_key_wise() {
        let home = Scratch::new("layered");
        home.write(
            "a11y.toml",
            "[contrast]\nmin = 7.0\n\n[theme]\ndark = \"Nord\"\n",
        );
        home.write("cms-web.a11y.toml", "[theme]\ndark = \"Dracula\"\n");

        let effective = complete_in(&home.0, Some("cms-web")).unwrap();
        assert_eq!(effective.theme.dark, "Dracula");
        assert_eq!(effective.contrast.min, 7.0, "the shared floor SURVIVES");
        assert_eq!(effective.theme.light, DEFAULT_LIGHT);
        assert_eq!(effective.layers.len(), 2, "both layers recorded");

        // Another app sees the shared file only.
        let other = complete_in(&home.0, Some("dev-server")).unwrap();
        assert_eq!(other.theme.dark, "Nord");
        assert_eq!(other.contrast.min, 7.0);
        assert_eq!(other.layers.len(), 1);

        // And no app at all sees the shared file only, too.
        let shared = complete_in(&home.0, None).unwrap();
        assert_eq!(shared.theme.dark, "Nord");
    }

    #[test]
    fn a_bad_file_fails_loudly_and_names_itself() {
        let home = Scratch::new("bad");
        home.write("a11y.toml", "[theme]\ndark = \"Base16OceanDrak\"\n");
        let err = complete_in(&home.0, None).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownTheme { .. }), "{err:?}");
        assert_eq!(
            A11y::default().theme.dark,
            DEFAULT_DARK,
            "no silent fallback"
        );
    }

    /// The ungated read reaches the rendering half of the very same files and
    /// stops there. This is the whole point of the split: `presentation_in` is
    /// what a consumer deriving an artifact calls, and no capability is checked
    /// on the way — so what it can reach has to be what it is entitled to.
    #[test]
    fn the_ungated_read_gets_the_rendering_half_of_the_same_files() {
        let home = Scratch::new("halves");
        home.write(
            "a11y.toml",
            "[contrast]\nmin = 7.0\n\n[theme]\ndark = \"Nord\"\n\n\
             [motion]\nreduce = true\n\n[text]\nscale = 1.75\nunderline_links = false\n",
        );

        let rendering = presentation_in(&home.0, None).unwrap();
        assert_eq!(rendering.theme.dark, "Nord");
        assert_eq!(rendering.contrast.min, 7.0);
        assert!(!rendering.underline_links);

        // The person-facts ARE in the file, and the whole config sees them.
        let whole = complete_in(&home.0, None).unwrap();
        assert_eq!(whole.motion.reduce, Some(true));
        assert_eq!(whole.text.scale, 1.75);

        // The ungated view cannot state them in any face it has.
        for face in [
            rendering.to_toml(),
            rendering.to_json(),
            rendering.to_turtle("urn:a11y:presentation", None),
        ] {
            assert!(!face.contains("reduce"), "{face}");
            assert!(!face.contains("scale"), "{face}");
            assert!(!face.contains("1.75"), "{face}");
        }
    }

    /// The threads name every CANDIDATE, so creating an override invalidates a
    /// cached answer that was computed before the file existed.
    #[test]
    fn the_threads_name_candidates_that_do_not_exist_yet() {
        let home = Path::new("/cfg/ikigai");
        let named: Vec<String> = layered_paths_in(home, STEM, Some("cms-web"))
            .iter()
            .map(|p| file_iri(p))
            .collect();
        assert_eq!(
            named,
            vec![
                "urn:file:/cfg/ikigai/a11y.toml".to_string(),
                "urn:file:/cfg/ikigai/cms-web.a11y.toml".to_string(),
            ]
        );
    }

    /// The ambient helpers agree with the rooted ones on whatever config home
    /// this machine has — asserted against the rule, since the environment is
    /// not the test's to pin.
    #[test]
    fn the_ambient_helpers_match_the_rooted_rule() {
        match config_home() {
            Some(home) => {
                assert_eq!(
                    paths(Some("cms-web")).unwrap(),
                    layered_paths_in(&home, STEM, Some("cms-web"))
                );
                assert_eq!(threads(None).unwrap().len(), 1);
            }
            None => {
                assert!(matches!(paths(None), Err(ConfigError::NoConfigHome)));
            }
        }
    }
}
