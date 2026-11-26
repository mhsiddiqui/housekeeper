//! Recipes: one reviewed TOML file per script-installed tool in this crate's `recipes/` folder, saying how to find the
//! tool and how to remove it (decisions D8, D26).
//!
//! The files are compiled into the binary; `build.rs` lists the folder, so adding a tool is one file and no Rust. A
//! recipe can only use a fixed vocabulary: find the install by a marker file under candidate folders, and remove it
//! by running its own uninstaller (a program inside the install) or by moving paths inside the install to the Trash.
//! It never runs a shell, and nothing it names can leave the install folder.
//!
//! ```toml
//! # recipes/rustup.toml; the file name is the tool's key (`installer/rustup`).
//! title = "rustup"
//!
//! [detect]
//! roots = [{ var = "RUSTUP_HOME", default = ".rustup" }]  # or { home = ".atuin" }, or { path = "/opt/x" }
//! marker = "bin/rustup"                                     # relative to the root, or to marker_root
//! marker_root = { var = "CARGO_HOME", default = ".cargo" }
//!
//! [remove]
//! uninstaller = { program = "bin/rustup", from = "marker-root", args = ["self", "uninstall", "-y"] }
//! # or: trash = ["."]  (paths inside the install root; "." is the root itself)
//! ```

use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use housekeeper_core::{Actor, Host, RemovalPlan, Step, Tier};
use serde::Deserialize;

include!(concat!(env!("OUT_DIR"), "/recipes.rs"));

/// One script-installed tool.
#[derive(Debug)]
pub(crate) struct Recipe {
    /// The file name without `.toml`: `rustup`.
    pub key: &'static str,
    pub title: String,
    roots: Vec<Root>,
    marker: PathBuf,
    marker_root: Option<Root>,
    removal: Option<Removal>,
}

/// A folder a tool may live in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Root {
    /// An environment variable holding an absolute path, falling back to a path under the home folder.
    Var {
        var: String,
        default: PathBuf,
    },
    /// A path under the home folder.
    Home(PathBuf),
    Absolute(PathBuf),
}

impl Root {
    fn path(&self, host: &Host) -> PathBuf {
        match self {
            Self::Var { var, default } => host.dir_from_var(var, &default.to_string_lossy()),
            Self::Home(relative) => host.home().join(relative),
            Self::Absolute(path) => path.clone(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Removal {
    /// The tool's own uninstaller: `program` inside the install, with `args`.
    Uninstaller {
        program: PathBuf,
        from_marker_root: bool,
        args: Vec<String>,
    },
    /// Paths inside the install root to move to the Trash.
    Trash(Vec<PathBuf>),
}

/// The recipes compiled in, parsed once.
static RECIPES: LazyLock<Vec<Result<Recipe, String>>> = LazyLock::new(|| {
    RECIPE_FILES
        .iter()
        .map(|(key, text)| parse(key, text).map_err(|error| format!("recipes/{key}.toml: {error}")))
        .collect()
});

/// Every recipe compiled in, in file-name order, or why one could not be read (`recipes/x.toml: …`). The built-in
/// files are checked by the tests, so an error here means a recipe was edited without running them.
pub(crate) fn recipes() -> &'static [Result<Recipe, String>] {
    &RECIPES
}

impl Recipe {
    /// The install folder: the first root that is a folder and has the marker.
    pub fn detect(&self, host: &Host) -> Option<PathBuf> {
        self.roots.iter().map(|root| root.path(host)).find(|root| {
            let base = self
                .marker_root
                .as_ref()
                .map_or_else(|| root.clone(), |base| base.path(host));
            host.is_dir(root) && host.exists(base.join(&self.marker))
        })
    }

    /// How the install in `root` is removed. Without a `[remove]` section nothing records how, so it is unknown.
    pub fn plan(&self, host: &Host, root: &Path) -> RemovalPlan {
        let home = host.home();
        match &self.removal {
            None => RemovalPlan::unknown(),
            Some(Removal::Uninstaller {
                program,
                from_marker_root,
                args,
            }) => {
                let base = match (&self.marker_root, from_marker_root) {
                    (Some(marker_root), true) => marker_root.path(host),
                    _ => root.to_path_buf(),
                };
                let program = base.join(program);
                let argv = std::iter::once(program.to_string_lossy().into_owned()).chain(args.iter().cloned());
                // A tool's own uninstaller runs as the user who installed it; the root helper only runs the system's
                // package tool and file actions (D25).
                RemovalPlan::new(Tier::OwnUninstaller, vec![Step::run(Actor::User, argv)])
            }
            Some(Removal::Trash(paths)) => {
                let mut plan = RemovalPlan::new(Tier::Recipe, Vec::new());
                // `.` is stored empty; joining it would add a trailing slash.
                let paths = paths.iter().map(|relative| {
                    if relative.as_os_str().is_empty() {
                        root.to_path_buf()
                    } else {
                        root.join(relative)
                    }
                });
                for path in paths {
                    // A variable can point the root at the home folder or above it; never trash those.
                    if path == Path::new("/") || home.starts_with(&path) {
                        plan = plan.blocked(format!(
                            "the recipe would trash {}, which holds your home folder",
                            path.display()
                        ));
                        continue;
                    }
                    plan.steps.push(Step::trash(actor(home, &path), path));
                }
                plan
            }
        }
    }
}

/// The user for paths in their home folder; the root helper for system paths (D25).
fn actor(home: &Path, path: &Path) -> Actor {
    if path.starts_with(home) && path != home {
        Actor::User
    } else {
        Actor::Helper
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecipeFile {
    title: String,
    detect: DetectSection,
    remove: Option<RemoveSection>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectSection {
    roots: Vec<RootSpec>,
    marker: String,
    marker_root: Option<RootSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RootSpec {
    var: Option<String>,
    default: Option<String>,
    home: Option<String>,
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveSection {
    uninstaller: Option<UninstallerSpec>,
    trash: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UninstallerSpec {
    program: String,
    #[serde(default)]
    from: From,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum From {
    #[default]
    Root,
    MarkerRoot,
}

fn parse(key: &'static str, text: &str) -> Result<Recipe, String> {
    let file: RecipeFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.detect.roots.is_empty() {
        return Err("detect.roots is empty".into());
    }
    let roots = file.detect.roots.into_iter().map(root).collect::<Result<Vec<_>, _>>()?;
    let marker_root = file.detect.marker_root.map(root).transpose()?;
    let removal = match file.remove {
        None => None,
        Some(RemoveSection {
            uninstaller: Some(uninstaller),
            trash: None,
        }) => {
            if uninstaller.from == From::MarkerRoot && marker_root.is_none() {
                return Err("remove.uninstaller.from is marker-root, but detect has no marker_root".into());
            }
            Some(Removal::Uninstaller {
                program: inside("remove.uninstaller.program", &uninstaller.program)?,
                from_marker_root: uninstaller.from == From::MarkerRoot,
                args: uninstaller.args,
            })
        }
        Some(RemoveSection {
            uninstaller: None,
            trash: Some(paths),
        }) if !paths.is_empty() => Some(Removal::Trash(
            paths
                .iter()
                .map(|path| inside("remove.trash", path))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Some(_) => return Err("[remove] needs exactly one of uninstaller or a non-empty trash list".into()),
    };
    Ok(Recipe {
        key,
        title: file.title,
        roots,
        marker: inside("detect.marker", &file.detect.marker)?,
        marker_root,
        removal,
    })
}

fn root(spec: RootSpec) -> Result<Root, String> {
    match spec {
        RootSpec {
            var: Some(var),
            default: Some(default),
            home: None,
            path: None,
        } => Ok(Root::Var {
            var,
            default: inside("a root's default", &default)?,
        }),
        RootSpec {
            var: None,
            default: None,
            home: Some(home),
            path: None,
        } => Ok(Root::Home(inside("a root's home", &home)?)),
        RootSpec {
            var: None,
            default: None,
            home: None,
            path: Some(path),
        } if Path::new(&path).is_absolute() && !has_parent(Path::new(&path)) => Ok(Root::Absolute(PathBuf::from(path))),
        _ => Err("a root is { var, default }, { home } or { path } (absolute)".into()),
    }
}

/// A path that stays inside the folder it is relative to: not absolute, no `..`. `.` is the folder itself.
fn inside(what: &str, path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    if path.is_empty() || relative.is_absolute() || has_parent(relative) {
        return Err(format!("{what} must be a relative path without `..`: {path:?}"));
    }
    Ok(relative.components().filter(|c| *c != Component::CurDir).collect())
}

fn has_parent(path: &Path) -> bool {
    path.components().any(|component| component == Component::ParentDir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use housekeeper_core::Action;
    use housekeeper_core::testing::{FakeRunner, Fixture, HOME};

    fn recipe(key: &str) -> &'static Recipe {
        recipes()
            .iter()
            .filter_map(|recipe| recipe.as_ref().ok())
            .find(|recipe| recipe.key == key)
            .unwrap()
    }

    #[test]
    fn every_built_in_recipe_is_valid() {
        let errors: Vec<&String> = recipes().iter().filter_map(|recipe| recipe.as_ref().err()).collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(recipes().len() >= 10);
    }

    #[test]
    fn recipes_cannot_leave_their_folder_or_use_unknown_actions() {
        let detect = "[detect]\nroots = [{ home = \".tool\" }]\nmarker = \"bin/tool\"\n";
        let cases = [
            (
                "title = \"x\"\n[detect]\nroots = [{ home = \"../etc\" }]\nmarker = \"m\"\n",
                "relative path",
            ),
            (
                "title = \"x\"\n[detect]\nroots = [{ path = \"opt\" }]\nmarker = \"m\"\n",
                "absolute",
            ),
            (
                &format!("title = \"x\"\n{detect}[remove]\ntrash = [\"../..\"]\n"),
                "relative path",
            ),
            (
                &format!("title = \"x\"\n{detect}[remove]\ntrash = [\"/etc\"]\n"),
                "relative path",
            ),
            (
                &format!("title = \"x\"\n{detect}[remove]\nrun = \"rm -rf ~\"\n"),
                "unknown field",
            ),
            (&format!("title = \"x\"\n{detect}[remove]\ntrash = []\n"), "exactly one"),
            (
                &format!(
                    "title = \"x\"\n{detect}[remove]\nuninstaller = {{ program = \"bin/tool\", from = \"marker-root\" }}\n"
                ),
                "marker_root",
            ),
        ];
        for (text, expected) in cases {
            let error = parse("x", text).unwrap_err();
            assert!(error.contains(expected), "{text:?}: {error}");
        }
    }

    #[test]
    fn rustup_runs_its_own_uninstaller_from_cargo_home() {
        let fixture = Fixture::new();
        fixture.dir(&format!("{HOME}/.rustup/toolchains"));
        fixture.executable("/srv/cargo/bin/rustup", "");
        let host = fixture.host_with_vars(FakeRunner::new(), &[("CARGO_HOME", "/srv/cargo")]);
        let rustup = recipe("rustup");
        let root = rustup.detect(&host).unwrap();
        assert_eq!(root, PathBuf::from(format!("{HOME}/.rustup")));
        let plan = rustup.plan(&host, &root);
        assert_eq!(plan.tier, Tier::OwnUninstaller);
        assert_eq!(
            plan.steps,
            [Step::run(
                Actor::User,
                ["/srv/cargo/bin/rustup", "self", "uninstall", "-y"]
            )]
        );
    }

    #[test]
    fn trash_recipes_trash_the_install_and_never_the_home_folder() {
        let fixture = Fixture::new();
        fixture.executable(&format!("{HOME}/.bun/bin/bun"), "");
        let host = fixture.host(FakeRunner::new());
        let bun = recipe("bun");
        let root = bun.detect(&host).unwrap();
        let plan = bun.plan(&host, &root);
        assert_eq!(plan.tier, Tier::Recipe);
        assert_eq!(plan.steps, [Step::trash(Actor::User, format!("{HOME}/.bun"))]);
        assert!(
            matches!(&plan.steps[0].action, Action::Trash { path } if path.to_str() == Some(&format!("{HOME}/.bun"))),
            "shown without a trailing slash: {:?}",
            plan.steps
        );
        assert!(plan.blockers.is_empty());

        // BUN_INSTALL pointing at the home folder itself.
        let plan = bun.plan(&host, Path::new(HOME));
        assert!(plan.steps.is_empty());
        assert_eq!(plan.blockers.len(), 1, "{:?}", plan.blockers);

        // An install outside the home folder is trashed by the root helper.
        let plan = recipe("nvm").plan(&host, Path::new("/srv/node"));
        assert_eq!(plan.steps, [Step::trash(Actor::Helper, "/srv/node")]);
    }

    #[test]
    fn a_tool_without_a_removal_recipe_is_unknown() {
        let host = Fixture::new().host(FakeRunner::new());
        assert_eq!(
            recipe("homebrew").plan(&host, Path::new("/home/linuxbrew/.linuxbrew")),
            RemovalPlan::unknown()
        );
    }
}
