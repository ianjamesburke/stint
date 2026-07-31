//! Task ID allocation: one shared ledger per repository, plus a survey of
//! every other place an ID might already be claimed.
//!
//! Each git worktree carries its own copy of `.stint/tasks/`, so a task file
//! in one worktree is structurally invisible to every other worktree until it
//! is committed and merged. Numbering off task files therefore cannot be made
//! correct, and requiring `.stint/` to be git-tracked would be the wrong
//! dependency — a repo that deliberately keeps its plan out of git still
//! deserves collision-free numbering.
//!
//! So allocation is owned by a ledger that lives outside every worktree, in
//! the common git directory, shared by all worktrees of the repository. See
//! [`Ledger`]. Reserving an ID is an exclusive `create_new` there, which is
//! the entire mutual-exclusion mechanism.
//!
//! [`IdSpace`] is the reconciliation half: it scans every worktree's task
//! directory on disk and every git ref's committed tree, so a ledger that is
//! missing or behind — a first run on an existing repo, or hand-created task
//! files — heals instead of handing out an ID something else already uses.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context};

/// Where a claim on an ID was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    /// A file in this working tree's `.stint/tasks/`.
    Working,
    /// A file on disk in another git worktree.
    Worktree(PathBuf),
    /// A file committed on a git ref (branch or remote-tracking branch).
    Ref(String),
    /// An entry in the shared ID allocation ledger.
    Ledger,
}

impl std::fmt::Display for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Origin::Working => write!(f, "this working tree"),
            Origin::Worktree(path) => write!(f, "worktree {}", path.display()),
            Origin::Ref(name) => write!(f, "ref {name}"),
            Origin::Ledger => write!(f, "the id allocation ledger"),
        }
    }
}

/// One claim on a task ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Claim {
    /// Task filename (`0016-some-slug.md`), or the bare ID for ledger claims.
    pub filename: String,
    /// Where the claim was found.
    pub origin: Origin,
}

impl std::fmt::Display for Claim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} in {}", self.filename, self.origin)
    }
}

/// Every ID claim the tool can see, plus reasons the view may be partial.
#[derive(Debug, Default, Clone)]
pub struct IdSpace {
    /// ID → every claim on it, deduplicated and sorted.
    pub claims: BTreeMap<String, BTreeSet<Claim>>,
    /// Human-readable reasons this survey may not see every claimed ID.
    pub warnings: Vec<String>,
}

impl IdSpace {
    /// Survey every ID claim reachable from `stint_dir`.
    ///
    /// Never fails on git problems: a repo with no git, a detached checkout or
    /// a missing `git` binary degrades to a working-directory-only survey plus
    /// a warning, because refusing to create tasks would be worse.
    pub fn survey(stint_dir: &Path, ledger: &Ledger) -> IdSpace {
        let mut space = IdSpace::default();

        space.add_dir(&stint_dir.join("tasks"), Origin::Working);
        space.add_ledger(ledger);

        let git = match GitView::discover(stint_dir) {
            Ok(Some(git)) => git,
            Ok(None) => {
                space.warnings.push(
                    "not a git repository: task files on other branches and worktrees cannot be \
                     surveyed; allocation still relies on the local ledger"
                        .to_owned(),
                );
                return space;
            }
            Err(error) => {
                space.warnings.push(format!(
                    "git unavailable ({error}): ids on other branches and worktrees are invisible"
                ));
                return space;
            }
        };

        space.add_git(&git);
        space
    }

    /// Insert every task file found directly in `dir`.
    fn add_dir(&mut self, dir: &Path, origin: Origin) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(id) = id_from_filename(&name) {
                self.insert(
                    id,
                    Claim {
                        filename: name,
                        origin: origin.clone(),
                    },
                );
            }
        }
    }

    fn add_ledger(&mut self, ledger: &Ledger) {
        for id in ledger.ids() {
            self.insert(
                id.clone(),
                Claim {
                    filename: id,
                    origin: Origin::Ledger,
                },
            );
        }
        if !ledger.shared {
            self.warnings.push(format!(
                "no git repository: the id ledger at {} is only shared with processes using that \
                 same directory",
                ledger.dir.display()
            ));
        }
    }

    fn add_git(&mut self, git: &GitView) {
        for worktree in &git.worktrees {
            if worktree.tasks_dir == git.tasks_dir {
                continue;
            }
            self.add_dir(&worktree.tasks_dir, Origin::Worktree(worktree.root.clone()));
        }

        for reference in &git.refs {
            match git.tracked_task_files(reference) {
                Ok(files) => {
                    for name in files {
                        if let Some(id) = id_from_filename(&name) {
                            self.insert(
                                id,
                                Claim {
                                    filename: name,
                                    origin: Origin::Ref(reference.clone()),
                                },
                            );
                        }
                    }
                }
                Err(error) => self
                    .warnings
                    .push(format!("cannot read task files on {reference}: {error}")),
            }
        }

        self.warnings.extend(git.completeness_warnings());
    }

    fn insert(&mut self, id: String, claim: Claim) {
        self.claims.entry(id).or_default().insert(claim);
    }

    /// Highest claimed ID as a number, or 0 when nothing is claimed.
    pub fn max_id(&self) -> u32 {
        self.claims
            .keys()
            .filter_map(|id| id.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
    }

    /// Every claim on `id`, in a stable order.
    pub fn claims_on(&self, id: &str) -> Vec<&Claim> {
        match self.claims.get(id) {
            Some(claims) => claims.iter().collect(),
            None => vec![],
        }
    }

    /// Whether anything anywhere already claims `id`.
    pub fn is_claimed(&self, id: &str) -> bool {
        self.claims.contains_key(id)
    }

    /// IDs claimed by more than one distinct task filename — a live collision.
    pub fn collisions(&self) -> Vec<(String, Vec<&Claim>)> {
        self.claims
            .iter()
            .filter_map(|(id, claims)| {
                let filenames: BTreeSet<&str> = claims
                    .iter()
                    .filter(|claim| claim.origin != Origin::Ledger)
                    .map(|claim| claim.filename.as_str())
                    .collect();
                if filenames.len() > 1 {
                    Some((id.clone(), claims.iter().collect()))
                } else {
                    None
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Ledger
// ---------------------------------------------------------------------------

/// The append-only record of every ID this repository has ever handed out.
///
/// One ledger per repository, shared by every worktree, living in the common
/// git directory (`git rev-parse --git-common-dir`) — the one place all
/// worktrees of a repo agree on and which no branch checkout can change. Task
/// files cannot play this role: each worktree carries its own copy of
/// `.stint/tasks/`, so a task file in one worktree is structurally invisible
/// to the others until it is committed and merged.
///
/// Entries are zero-byte files created with `O_EXCL`. They are never removed,
/// so an ID stays spent even if its task file is deleted, renamed, or never
/// committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger {
    /// Directory holding one file per allocated ID.
    pub dir: PathBuf,
    /// Whether this ledger is shared by every worktree of the repository.
    ///
    /// False only outside git, where the fallback ledger lives in `.stint/`
    /// and can only serialise processes sharing that directory.
    pub shared: bool,
}

impl Ledger {
    /// Locate the ledger for the repository containing `stint_dir`.
    pub fn locate(stint_dir: &Path) -> Ledger {
        let parent = stint_dir.parent().unwrap_or(stint_dir);
        let common_dir = git_stdout(
            parent,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .ok()
        .flatten()
        .map(|out| PathBuf::from(out.trim_end_matches('\n').to_owned()))
        .filter(|path| path.is_dir());

        match common_dir {
            Some(git_dir) => Ledger {
                dir: git_dir.join("stint").join("ids"),
                shared: true,
            },
            None => Ledger {
                dir: stint_dir.join("ids"),
                shared: false,
            },
        }
    }

    /// Every ID currently recorded in the ledger.
    pub fn ids(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return vec![];
        };
        entries
            .flatten()
            .filter_map(|entry| canonical_id(&entry.file_name().to_string_lossy()))
            .collect()
    }

    /// Try to claim `id`.
    ///
    /// `Ok(true)` when this call created the entry, `Ok(false)` when another
    /// process already holds it. The exclusive create is the whole mutual
    /// exclusion mechanism: two agents filing at the same instant cannot both
    /// see `true`.
    pub fn reserve(&self, id: &str) -> anyhow::Result<bool> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("create ledger {}", self.dir.display()))?;
        let path = self.dir.join(id);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error).with_context(|| format!("reserve {}", path.display())),
        }
    }

    /// Record every ID in `space` that the ledger does not yet know about.
    ///
    /// Self-healing for a repository whose task files predate the ledger, or
    /// where somebody hand-created a task file. Returns the IDs adopted.
    pub fn reconcile(&self, space: &IdSpace) -> anyhow::Result<Vec<String>> {
        let mut adopted = Vec::new();
        for id in space.claims.keys() {
            if self.reserve(id)? {
                adopted.push(id.clone());
            }
        }
        Ok(adopted)
    }
}

// ---------------------------------------------------------------------------
// Allocation
// ---------------------------------------------------------------------------

/// Reserve the next free task ID.
///
/// Starts one past the highest ID seen anywhere — ledger, any worktree's task
/// files, any git ref — and walks forward, retrying whenever another process
/// wins the exclusive create.
pub fn allocate_next_id(ledger: &Ledger, space: &IdSpace) -> anyhow::Result<String> {
    for n in space.max_id() + 1..=9999 {
        let id = format!("{n:04}");
        if ledger.reserve(&id)? {
            return Ok(id);
        }
    }
    bail!("no free task id below 9999; the id space is exhausted")
}

/// Reserve a caller-supplied ID, refusing loudly if it is already claimed.
pub fn allocate_explicit_id(ledger: &Ledger, space: &IdSpace, id: &str) -> anyhow::Result<String> {
    let id = canonical_id(id)
        .ok_or_else(|| anyhow::anyhow!("invalid task id {id:?}: expected up to 4 digits"))?;

    if space.is_claimed(&id) {
        let claims = space
            .claims_on(&id)
            .iter()
            .map(|claim| format!("  {claim}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!("task id {id} is already claimed:\n{claims}");
    }
    if !ledger.reserve(&id)? {
        bail!(
            "task id {id} is already claimed:\n  {id} in {}",
            Origin::Ledger
        );
    }
    Ok(id)
}

// ---------------------------------------------------------------------------
// Filename / id helpers
// ---------------------------------------------------------------------------

/// Extract the canonical 4-digit ID from a task filename, if it has one.
///
/// `"0016-some-slug.md"` → `Some("0016")`, `"README.md"` → `None`.
pub fn id_from_filename(filename: &str) -> Option<String> {
    let stem = filename.strip_suffix(".md")?;
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &stem[digits.len()..];
    if !rest.is_empty() && !rest.starts_with('-') {
        return None;
    }
    canonical_id(&digits)
}

/// Normalise a numeric ID string to its canonical zero-padded form.
fn canonical_id(input: &str) -> Option<String> {
    if input.is_empty() || input.len() > 4 || !input.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: u32 = input.parse().ok()?;
    Some(format!("{n:04}"))
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

/// A worktree of the enclosing repository.
struct WorktreeView {
    root: PathBuf,
    tasks_dir: PathBuf,
}

/// Everything the survey needs to read out of git.
struct GitView {
    toplevel: PathBuf,
    /// `.stint/tasks` relative to the repository toplevel, slash-separated.
    relative_tasks_dir: String,
    tasks_dir: PathBuf,
    refs: Vec<String>,
    worktrees: Vec<WorktreeView>,
}

impl GitView {
    /// Returns `Ok(None)` when `stint_dir` is not inside a git repository.
    fn discover(stint_dir: &Path) -> anyhow::Result<Option<GitView>> {
        let parent = stint_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", stint_dir.display()))?;
        let Some(toplevel) = git_stdout(parent, &["rev-parse", "--show-toplevel"])? else {
            return Ok(None);
        };
        let toplevel = PathBuf::from(toplevel.trim_end_matches('\n'));

        let stint_dir = fs::canonicalize(stint_dir).unwrap_or_else(|_| stint_dir.to_path_buf());
        let canonical_toplevel = fs::canonicalize(&toplevel).unwrap_or_else(|_| toplevel.clone());
        let relative = stint_dir
            .strip_prefix(&canonical_toplevel)
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|_| PathBuf::from(".stint"));
        let relative_tasks_dir = relative
            .join("tasks")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");

        let refs = git_stdout(
            &toplevel,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
            ],
        )?
        .unwrap_or_default()
        .lines()
        .map(|line| line.to_owned())
        .filter(|line| !line.is_empty())
        .collect();

        let worktrees = git_stdout(&toplevel, &["worktree", "list", "--porcelain"])?
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(|path| {
                let root = PathBuf::from(path);
                let tasks_dir = root.join(&relative).join("tasks");
                WorktreeView { root, tasks_dir }
            })
            .collect();

        Ok(Some(GitView {
            toplevel,
            relative_tasks_dir,
            tasks_dir: stint_dir.join("tasks"),
            refs,
            worktrees,
        }))
    }

    /// Task filenames committed under the tasks directory on `reference`.
    fn tracked_task_files(&self, reference: &str) -> anyhow::Result<Vec<String>> {
        let output = git_stdout(
            &self.toplevel,
            &[
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                reference,
                "--",
                &self.relative_tasks_dir,
            ],
        )?
        .unwrap_or_default();
        Ok(basenames_nul_separated(&output))
    }

    /// Reasons the ID space this survey saw may still be partial.
    fn completeness_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if git_succeeds(
            &self.toplevel,
            &["check-ignore", "-q", &self.relative_tasks_dir],
        ) {
            warnings.push(format!(
                "{} is git-ignored: ids stay unique via the shared ledger, but the tasks \
                 themselves never reach another clone of this repo",
                self.relative_tasks_dir
            ));
            return warnings;
        }

        let head = match self.tracked_task_files("HEAD") {
            Ok(files) => files.into_iter().collect::<BTreeSet<_>>(),
            Err(_) => return warnings,
        };

        let mut uncommitted: Vec<String> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.tasks_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if id_from_filename(&name).is_some() && !head.contains(&name) {
                    uncommitted.push(name);
                }
            }
        }
        uncommitted.sort();

        if !uncommitted.is_empty() {
            warnings.push(format!(
                "{} task file(s) are not committed on HEAD, so the tasks are invisible to other \
                 clones (their ids are still safe): {}{}. Commit them.",
                uncommitted.len(),
                uncommitted
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                if uncommitted.len() > 5 { ", …" } else { "" }
            ));
        }

        warnings
    }
}

/// Run git in `dir`, returning stdout, or `None` when git exits non-zero.
fn git_stdout(dir: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("run git {}", args.join(" ")))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Split NUL-separated git paths and keep only the final path component.
fn basenames_nul_separated(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| entry.rsplit('/').next())
        .map(|name| name.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_from_filename_accepts_slugged_and_bare() {
        assert_eq!(
            id_from_filename("0016-some-slug.md").as_deref(),
            Some("0016")
        );
        assert_eq!(id_from_filename("16.md").as_deref(), Some("0016"));
        assert_eq!(id_from_filename("0016.md").as_deref(), Some("0016"));
    }

    #[test]
    fn id_from_filename_rejects_non_tasks() {
        assert_eq!(id_from_filename("README.md"), None);
        assert_eq!(id_from_filename("0016-some-slug.txt"), None);
        assert_eq!(id_from_filename("0016abc.md"), None);
    }

    #[test]
    fn basenames_strips_directories() {
        let output = ".stint/tasks/0001-a.md\0.stint/tasks/0002-b.md\0";
        assert_eq!(
            basenames_nul_separated(output),
            vec!["0001-a.md", "0002-b.md"]
        );
    }

    #[test]
    fn basenames_handles_spaces_in_paths() {
        let output = ".stint/tasks/0001-a b.md\0";
        assert_eq!(basenames_nul_separated(output), vec!["0001-a b.md"]);
    }
}
