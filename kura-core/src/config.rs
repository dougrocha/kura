//! The config file the CLI, the TUI and the Mac app share:
//! `~/.config/kura/config.toml` (or `$XDG_CONFIG_HOME/kura`, or `KURA_CONFIG`).
//!
//! ```toml
//! library = "~/Memes"            # unset: ~/Pictures/kura and the system folders
//!
//! [pick]
//! close_after_copy = true        # Mac app: hide Kura; `kura pick`: exit
//!
//! [mac]
//! relaunch_when_hidden = true    # the CLI ignores [mac]
//! ```
//!
//! Edits go through `toml_edit`, so comments and unknown keys survive
//! `kura config set` and the Mac app's Settings.

use std::path::{Path, PathBuf};

use directories::BaseDirs;
use toml_edit::{DocumentMut, Item, Table, TableLike, value};

/// Every setting, parsed, with defaults filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// A library folder, like `--library`. None: the default library.
    pub library: Option<PathBuf>,
    pub close_after_copy: bool,
    pub relaunch_when_hidden: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { library: None, close_after_copy: true, relaunch_when_hidden: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyKind {
    Path,
    Bool,
}

/// One setting, as `kura config` and the server name it.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    pub name: &'static str,
    pub(crate) kind: KeyKind,
    pub about: &'static str,
}

pub const KEYS: &[Key] = &[
    Key {
        name: "library",
        kind: KeyKind::Path,
        about: "Keep the whole library in this folder. Unset: ~/Pictures/kura",
    },
    Key {
        name: "pick.close_after_copy",
        kind: KeyKind::Bool,
        about: "After copying, hide the Mac app and exit `kura pick`",
    },
    Key {
        name: "mac.relaunch_when_hidden",
        kind: KeyKind::Bool,
        about: "Mac app: relaunch into an update once Kura is hidden",
    },
];

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown setting '{0}'; `kura config` lists them")]
    UnknownKey(String),
    #[error("{key}: {message}")]
    InvalidValue { key: String, message: String },
    #[error("{path} isn't valid TOML: {message}")]
    Syntax { path: PathBuf, message: String },
    #[error("can't find your home folder")]
    NoHome,
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

type Result<T> = std::result::Result<T, ConfigError>;

/// The config file on disk. Reading a missing file gives the defaults.
#[derive(Debug, Clone)]
pub struct ConfigFile {
    path: PathBuf,
}

impl ConfigFile {
    /// `KURA_CONFIG`, else `$XDG_CONFIG_HOME/kura/config.toml`, else
    /// `~/.config/kura/config.toml`, on macOS too: easy to find and edit.
    pub fn locate() -> Result<Self> {
        let var = |name| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        if let Some(path) = var("KURA_CONFIG") {
            return Ok(Self::at(path));
        }
        let config_home = match var("XDG_CONFIG_HOME") {
            Some(dir) => dir,
            None => home()?.join(".config"),
        };
        Ok(Self::at(config_home.join("kura").join("config.toml")))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<Config> {
        let doc = self.document()?;
        let mut config = Config::default();
        if let Some(item) = lookup(&doc, "library") {
            let invalid = |message: &str| self.invalid("library", message);
            let raw = item.as_str().ok_or_else(|| invalid("expected a folder path in quotes"))?;
            config.library = Some(expand(raw).map_err(|e| invalid(&e))?);
        }
        if let Some(value) = self.bool_at(&doc, "pick.close_after_copy")? {
            config.close_after_copy = value;
        }
        if let Some(value) = self.bool_at(&doc, "mac.relaunch_when_hidden")? {
            config.relaunch_when_hidden = value;
        }
        Ok(config)
    }

    fn invalid(&self, key: &str, message: &str) -> ConfigError {
        ConfigError::InvalidValue { key: key.to_owned(), message: format!("{message} (in {})", self.path.display()) }
    }

    fn bool_at(&self, doc: &DocumentMut, name: &str) -> Result<Option<bool>> {
        let Some(item) = lookup(doc, name) else { return Ok(None) };
        let value = item.as_bool().ok_or_else(|| self.invalid(name, "expected true or false"))?;
        Ok(Some(value))
    }

    /// The value as written in the file (`None` when unset), for `kura config get`.
    pub fn get(&self, name: &str) -> Result<Option<String>> {
        key(name)?;
        let doc = self.document()?;
        Ok(lookup(&doc, name).and_then(|item| item.as_value()).map(|v| match v.as_str() {
            Some(s) => s.to_owned(),
            None => v.to_string().trim().to_owned(),
        }))
    }

    /// Set `name` from text, as typed after `kura config set`: `true`/`false`
    /// for switches, a folder (absolute or `~/…`) for paths.
    pub fn set(&self, name: &str, raw: &str) -> Result<()> {
        let key = key(name)?;
        let invalid = |message: String| ConfigError::InvalidValue { key: name.to_owned(), message };
        let new = match key.kind {
            KeyKind::Bool => match raw.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "on" => value(true),
                "false" | "no" | "off" => value(false),
                _ => return Err(invalid(format!("expected true or false, not '{raw}'"))),
            },
            KeyKind::Path => {
                expand(raw.trim()).map_err(invalid)?;
                value(raw.trim())
            }
        };
        let mut doc = self.document()?;
        // Parents as `[pick]` sections, not `pick = { … }`.
        let (parents, leaf) = name.rsplit_once('.').map_or(("", name), |(p, l)| (p, l));
        let mut table = doc.as_table_mut();
        for part in parents.split('.').filter(|p| !p.is_empty()) {
            let entry = table.entry(part).or_insert_with(|| Item::Table(Table::new()));
            table = entry.as_table_mut().ok_or_else(|| invalid(format!("'{part}' isn't a [section] in the file")))?;
        }
        table[leaf] = new;
        self.write(&doc)
    }

    /// Back to the default.
    pub fn unset(&self, name: &str) -> Result<()> {
        key(name)?;
        let mut doc = self.document()?;
        let (parent, leaf) = name.rsplit_once('.').map_or((None, name), |(p, l)| (Some(p), l));
        let table: Option<&mut dyn TableLike> = match parent {
            None => Some(doc.as_table_mut()),
            Some(parent) => doc.get_mut(parent).and_then(Item::as_table_like_mut),
        };
        if table.and_then(|t| t.remove(leaf)).is_some() {
            self.write(&doc)?;
        }
        Ok(())
    }

    /// A commented starting file, for `kura config edit` when there's none yet.
    pub fn template() -> String {
        "# Kura settings, shared by the CLI and the Mac app. Every line is optional.\n\n\
         # library = \"~/Memes\"\n\n\
         [pick]\n# close_after_copy = true\n\n\
         [mac]\n# relaunch_when_hidden = true\n"
            .to_owned()
    }

    /// Create the file from [`ConfigFile::template`] if it doesn't exist yet.
    pub fn create_template(&self) -> Result<()> {
        if self.path.exists() {
            return Ok(());
        }
        let doc: DocumentMut = Self::template().parse().map_err(|e: toml_edit::TomlError| ConfigError::Syntax {
            path: self.path.clone(),
            message: e.to_string(),
        })?;
        self.write(&doc)
    }

    fn document(&self) -> Result<DocumentMut> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => return Err(ConfigError::Io { path: self.path.clone(), source }),
        };
        text.parse()
            .map_err(|e: toml_edit::TomlError| ConfigError::Syntax { path: self.path.clone(), message: e.to_string() })
    }

    /// Write beside the file, then rename, so readers never see half of it.
    fn write(&self, doc: &DocumentMut) -> Result<()> {
        let io = |source| ConfigError::Io { path: self.path.clone(), source };
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let temp = self.path.with_extension("toml.tmp");
        std::fs::write(&temp, doc.to_string()).map_err(io)?;
        std::fs::rename(&temp, &self.path).map_err(io)
    }
}

fn key(name: &str) -> Result<&'static Key> {
    KEYS.iter().find(|k| k.name == name).ok_or_else(|| ConfigError::UnknownKey(name.to_owned()))
}

fn lookup<'a>(doc: &'a DocumentMut, name: &str) -> Option<&'a Item> {
    let mut item = doc.as_item();
    for part in name.split('.') {
        item = item.get(part)?;
    }
    Some(item).filter(|item| !item.is_none())
}

fn home() -> Result<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()).ok_or(ConfigError::NoHome)
}

/// `~` and `~/…` become the home folder; anything else must be absolute.
fn expand(raw: &str) -> std::result::Result<PathBuf, String> {
    let path = if raw == "~" {
        home().map_err(|e| e.to_string())?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home().map_err(|e| e.to_string())?.join(rest)
    } else {
        PathBuf::from(raw)
    };
    if raw.is_empty() || !path.is_absolute() {
        return Err(format!("expected an absolute folder or one starting with ~/, not '{raw}'"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> (tempfile::TempDir, ConfigFile) {
        let dir = tempfile::tempdir().unwrap();
        let file = ConfigFile::at(dir.path().join("kura").join("config.toml"));
        (dir, file)
    }

    #[test]
    fn missing_file_is_the_defaults() {
        let (_dir, file) = file();
        assert_eq!(file.load().unwrap(), Config::default());
        assert_eq!(file.get("library").unwrap(), None);
    }

    #[test]
    fn set_get_unset() {
        let (_dir, file) = file();
        file.set("pick.close_after_copy", "no").unwrap();
        file.set("library", "/tmp/memes").unwrap();
        assert_eq!(file.get("pick.close_after_copy").unwrap().as_deref(), Some("false"));
        assert_eq!(file.get("library").unwrap().as_deref(), Some("/tmp/memes"));
        let config = file.load().unwrap();
        assert!(!config.close_after_copy);
        assert_eq!(config.library, Some(PathBuf::from("/tmp/memes")));

        file.unset("pick.close_after_copy").unwrap();
        file.unset("library").unwrap();
        assert_eq!(file.load().unwrap(), Config::default());
    }

    #[test]
    fn keeps_comments_and_unknown_keys() {
        let (_dir, file) = file();
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        std::fs::write(file.path(), "# mine\nfuture = 1\n\n[mac]\nrelaunch_when_hidden = true # keep\n").unwrap();
        file.set("mac.relaunch_when_hidden", "false").unwrap();
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert!(text.starts_with("# mine\nfuture = 1\n"), "{text}");
        assert!(text.contains("relaunch_when_hidden = false"), "{text}");
    }

    #[test]
    fn rejects_bad_input() {
        let (_dir, file) = file();
        assert!(matches!(file.set("nope", "1"), Err(ConfigError::UnknownKey(_))));
        assert!(matches!(file.set("pick.close_after_copy", "maybe"), Err(ConfigError::InvalidValue { .. })));
        assert!(matches!(file.set("library", "relative/dir"), Err(ConfigError::InvalidValue { .. })));

        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        std::fs::write(file.path(), "[pick]\nclose_after_copy = \"yes\"\n").unwrap();
        assert!(matches!(file.load(), Err(ConfigError::InvalidValue { .. })));
        std::fs::write(file.path(), "library = [").unwrap();
        assert!(matches!(file.load(), Err(ConfigError::Syntax { .. })));
    }

    #[test]
    fn template_parses_to_the_defaults() {
        let (_dir, file) = file();
        std::fs::create_dir_all(file.path().parent().unwrap()).unwrap();
        std::fs::write(file.path(), ConfigFile::template()).unwrap();
        assert_eq!(file.load().unwrap(), Config::default());
    }
}
