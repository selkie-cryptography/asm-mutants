//! A copy of the source tree that mutants are applied to and built in.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use ignore::WalkBuilder;
use tempfile::TempDir;

use crate::{
    mutant::Mutant,
    source::{OUTPUT_DIRS, SourceTree, VCS_DIRS},
};

/// What to carry into the copy.
#[derive(Clone, Copy, Debug)]
pub struct CopyOptions {
    pub copy_target: bool,
    pub copy_vcs: bool,
    pub gitignore: bool,
    /// Keep the scratch directory after the run.
    pub leak: bool,
}

#[derive(Debug)]
pub struct BuildDir {
    path: PathBuf,
    /// Deletes the copy on drop. `None` for an in-place tree or a leaked
    /// copy.
    _temp: Option<TempDir>,
}

impl BuildDir {
    /// Mutate the source tree itself.
    pub fn in_place(tree: &SourceTree) -> Self {
        Self {
            path: tree.root.clone(),
            _temp: None,
        }
    }

    /// A fresh copy of the tree in the system temp directory.
    pub fn copy_from(tree: &SourceTree, options: CopyOptions) -> Result<Self> {
        let temp = tempfile::Builder::new()
            .prefix("asm-mutants-")
            .tempdir()
            .context("create scratch directory")?;
        let name = tree
            .root
            .file_name()
            .map_or_else(|| "tree".to_string(), |n| n.to_string_lossy().into_owned());
        let path = temp.path().join(name);
        Self::copy_tree(&tree.root, &path, options)?;
        if options.copy_target {
            let target = tree.root.join("target");
            if target.is_dir() {
                Self::copy_plain(&target, &path.join("target"))?;
            }
        }
        let temp = if options.leak {
            eprintln!("leaking scratch directory {}", temp.path().display());
            // The handle is dropped without deleting.
            let _ = temp.keep();
            None
        } else {
            Some(temp)
        };
        Ok(Self { path, _temp: temp })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Copies the tree minus build output, VCS state, our output, and
    /// (optionally) gitignored files.
    fn copy_tree(from: &Path, to: &Path, options: CopyOptions) -> Result<()> {
        let walk = WalkBuilder::new(from)
            .hidden(false)
            .require_git(false)
            .git_ignore(options.gitignore)
            .git_global(false)
            .git_exclude(false)
            .filter_entry(move |entry| {
                let name = entry.file_name().to_string_lossy();
                !(entry.depth() == 1
                    && (name == "target"
                        || OUTPUT_DIRS.contains(&name.as_ref())
                        || (!options.copy_vcs && VCS_DIRS.contains(&name.as_ref()))))
            })
            .build();
        for entry in walk {
            let entry = entry?;
            let relative = entry.path().strip_prefix(from).unwrap_or(entry.path());
            let dest = to.join(relative);
            if entry.file_type().is_some_and(|t| t.is_dir()) {
                fs::create_dir_all(&dest)?;
            } else {
                fs::copy(entry.path(), &dest)
                    .with_context(|| format!("copy {}", entry.path().display()))?;
            }
        }
        Ok(())
    }

    /// A recursive copy that ignores nothing, for `target/`.
    fn copy_plain(from: &Path, to: &Path) -> Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let dest = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                Self::copy_plain(&entry.path(), &dest)?;
            } else {
                fs::copy(entry.path(), &dest)?;
            }
        }
        Ok(())
    }

    /// Writes the mutated file into the copy.
    pub fn apply(&self, mutant: &Mutant) -> Result<()> {
        let path = self.path.join(&mutant.site.file.path);
        fs::write(&path, mutant.apply()).with_context(|| format!("write {}", path.display()))
    }

    /// Puts the original file back.
    pub fn restore(&self, mutant: &Mutant) -> Result<()> {
        let path = self.path.join(&mutant.site.file.path);
        fs::write(&path, &mutant.site.file.text)
            .with_context(|| format!("restore {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use std::process::id;

    use super::*;

    #[test]
    fn copies_without_target_vcs_or_output() {
        let root = std::env::temp_dir().join(format!("asm-mutants-build-dir-{}", id()));
        for dir in [
            "src",
            "target/debug",
            ".git/objects",
            "asm-mutants.out/log",
            ".cargo",
        ] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(root.join("target/debug/x"), "").unwrap();
        fs::write(root.join(".git/HEAD"), "").unwrap();
        fs::write(root.join(".cargo/config.toml"), "").unwrap();
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("ignored.txt"), "").unwrap();

        let tree = SourceTree::new(root.clone());
        let options = CopyOptions {
            copy_target: false,
            copy_vcs: false,
            gitignore: true,
            leak: false,
        };
        let copy = BuildDir::copy_from(&tree, options).unwrap();
        assert!(copy._temp.is_some());
        assert!(copy.path().join("src/lib.rs").exists());
        assert!(copy.path().join(".cargo/config.toml").exists());
        assert!(!copy.path().join("target").exists());
        assert!(!copy.path().join(".git").exists());
        assert!(!copy.path().join("asm-mutants.out").exists());
        assert!(!copy.path().join("ignored.txt").exists());

        let with_target = BuildDir::copy_from(
            &tree,
            CopyOptions {
                copy_target: true,
                gitignore: false,
                ..options
            },
        )
        .unwrap();
        assert!(with_target.path().join("target/debug/x").exists());
        assert!(with_target.path().join("ignored.txt").exists());

        fs::remove_dir_all(root).unwrap();
    }
}
