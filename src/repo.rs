/// Locate the `.stint/` directory, load tasks and sprints, resolve task IDs.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use stint::parse::{parse_gate, parse_task, ParseError};
use stint::schema::{Gate, Sprint, Task};
use stint::sprint::parse_sprint;

/// A handle to an initialised `.stint/` workspace.
pub struct StintRepo {
    /// The `.stint/` directory (e.g. `/my/project/.stint`).
    pub stint_dir: PathBuf,
}

impl StintRepo {
    /// Create a new `.stint/` workspace directly under `root`.
    pub fn init(root: &Path, force: bool) -> anyhow::Result<Self> {
        let stint_dir = root.join(".stint");
        if stint_dir.exists() && !force {
            bail!(
                "{} already exists; use --force to ensure missing files",
                stint_dir.display()
            );
        }

        let repo = StintRepo { stint_dir };
        repo.ensure_dirs()?;
        fs::create_dir_all(repo.gates_dir()).context("create .stint/gates/")?;

        let config_path = repo.stint_dir.join("config.toml");
        if !config_path.exists() {
            fs::write(
                &config_path,
                "[project]\nname = \"\"\ndefault_sprint = \"\"\n\n[gh]\nrepo = \"\"\n",
            )
            .with_context(|| format!("write {}", config_path.display()))?;
        }

        Ok(repo)
    }

    /// Walk up from `from` until a directory containing `.stint/` is found.
    pub fn find(from: &Path) -> anyhow::Result<Self> {
        let mut current = from.to_path_buf();
        loop {
            let candidate = current.join(".stint");
            if candidate.is_dir() {
                return Ok(StintRepo {
                    stint_dir: candidate,
                });
            }
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => bail!(
                    "no .stint/ directory found walking up from {}",
                    from.display()
                ),
            }
        }
    }

    /// Path to the tasks directory.
    pub fn tasks_dir(&self) -> PathBuf {
        self.stint_dir.join("tasks")
    }

    /// Path to the sprints directory.
    pub fn sprints_dir(&self) -> PathBuf {
        self.stint_dir.join("sprints")
    }

    /// Path to the gates directory.
    pub fn gates_dir(&self) -> PathBuf {
        self.stint_dir.join("gates")
    }

    /// Ensure the `tasks/` and `sprints/` sub-directories exist.
    pub fn ensure_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(self.tasks_dir()).context("create .stint/tasks/")?;
        fs::create_dir_all(self.sprints_dir()).context("create .stint/sprints/")?;
        Ok(())
    }

    pub fn load_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let (tasks, errors) = self.load_tasks_with_errors()?;
        for error in errors {
            eprintln!("warning: skip {}: {}", error.path.display(), error.error);
        }
        Ok(tasks)
    }

    /// Load and parse every task file in `.stint/tasks/`, returning parse
    /// errors so `stint check` can report them as failures.
    pub fn load_tasks_with_errors(&self) -> anyhow::Result<(Vec<Task>, Vec<TaskParseError>)> {
        let dir = self.tasks_dir();
        if !dir.exists() {
            return Ok((vec![], vec![]));
        }
        let mut tasks = Vec::new();
        let mut errors = Vec::new();
        for entry in fs::read_dir(&dir).context("read .stint/tasks/")? {
            let entry = entry.context("read dir entry")?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let filename = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("path has no filename: {}", path.display()))?
                .to_string_lossy()
                .into_owned();
            let content =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            match parse_task(&content, &filename) {
                Ok(task) => tasks.push(task),
                Err(error) => errors.push(TaskParseError { path, error }),
            }
        }
        // Sort by numeric ID for deterministic output.
        tasks.sort_by(|a, b| a.frontmatter.id.cmp(&b.frontmatter.id));
        errors.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((tasks, errors))
    }

    /// Load and parse every sprint file in `.stint/sprints/`.
    pub fn load_sprints(&self) -> anyhow::Result<Vec<Sprint>> {
        let dir = self.sprints_dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut sprints = Vec::new();
        for entry in fs::read_dir(&dir).context("read .stint/sprints/")? {
            let entry = entry.context("read dir entry")?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let content =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            match parse_sprint(&content) {
                Ok(sprint) => sprints.push(sprint),
                Err(e) => eprintln!("warning: skip {}: {}", path.display(), e),
            }
        }
        sprints.sort_by(|a, b| a.header.id.cmp(&b.header.id));
        Ok(sprints)
    }

    pub fn load_gates(&self) -> anyhow::Result<Vec<Gate>> {
        let (gates, errors) = self.load_gates_with_errors()?;
        for error in errors {
            eprintln!("warning: skip {}: {}", error.path.display(), error.error);
        }
        Ok(gates)
    }

    pub fn load_gates_with_errors(&self) -> anyhow::Result<(Vec<Gate>, Vec<TaskParseError>)> {
        let dir = self.gates_dir();
        if !dir.exists() {
            return Ok((vec![], vec![]));
        }
        let mut gates = Vec::new();
        let mut errors = Vec::new();
        for entry in fs::read_dir(&dir).context("read .stint/gates/")? {
            let entry = entry.context("read dir entry")?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let filename = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("path has no filename: {}", path.display()))?
                .to_string_lossy()
                .into_owned();
            let content =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            match parse_gate(&content, &filename) {
                Ok(gate) => gates.push(gate),
                Err(error) => errors.push(TaskParseError { path, error }),
            }
        }
        gates.sort_by(|a, b| a.id.cmp(&b.id));
        errors.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((gates, errors))
    }

    /// Resolve a user-supplied task ID fragment to a full path on disk.
    ///
    /// The numeric prefix is extracted, padded to 4 digits, then matched
    /// against filenames in `.stint/tasks/`.  Returns an error if zero or
    /// more than one match is found.
    pub fn resolve_task_path(&self, id_input: &str) -> anyhow::Result<PathBuf> {
        let id = stint::mutate::resolve_id(id_input);
        let dir = self.tasks_dir();
        let mut matches = Vec::new();
        if dir.exists() {
            for entry in fs::read_dir(&dir).context("read .stint/tasks/")? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&id) && name.ends_with(".md") {
                    // Verify the next char is either end-of-id or '-'
                    let rest = &name[id.len()..];
                    if rest.is_empty() || rest.starts_with('-') {
                        matches.push(entry.path());
                    }
                }
            }
        }
        match matches.len() {
            0 => bail!("no task found for id {:?}", id_input),
            1 => Ok(matches.remove(0)),
            _ => bail!("ambiguous id {:?}: multiple matches", id_input),
        }
    }

    /// Resolve a sprint ID to a file path.
    ///
    /// The sprint ID may omit the leading `s` (e.g. `"12"` → `"s12"`).
    pub fn resolve_sprint_path(&self, id_input: &str) -> anyhow::Result<PathBuf> {
        let id = stint::sprint::normalize_sprint_id(id_input);
        let path = self.sprints_dir().join(format!("{}.md", id));
        if path.exists() {
            Ok(path)
        } else {
            bail!("no sprint found for id {:?}", id_input)
        }
    }

    /// Read a task file by path and parse it.
    pub fn read_task(&self, path: &Path) -> anyhow::Result<Task> {
        let content =
            fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let filename = path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("path has no filename: {}", path.display()))?
            .to_string_lossy()
            .into_owned();
        parse_task(&content, &filename).with_context(|| format!("parse {}", path.display()))
    }

    /// Write a serialized task back to its file.
    pub fn write_task(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        fs::write(path, content).with_context(|| format!("write {}", path.display()))
    }

    /// Read the raw string content of a sprint file.
    pub fn read_sprint_raw(&self, path: &Path) -> anyhow::Result<String> {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
    }

    /// Write a sprint file.
    pub fn write_sprint(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        fs::write(path, content).with_context(|| format!("write {}", path.display()))
    }
}

/// A task file that could not be parsed.
pub struct TaskParseError {
    pub path: PathBuf,
    pub error: ParseError,
}
