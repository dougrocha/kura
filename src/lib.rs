use std::{fs, path::PathBuf};

use directories::{ProjectDirs, UserDirs};
use miette::{Context, IntoDiagnostic, Result, miette};
use rusqlite::Connection;

pub mod types;
pub mod images;
pub mod tags;

pub static PKG_NAME: &str = env!("CARGO_PKG_NAME");
pub static VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct State {
    pub picture_dir: PathBuf,
    pub data_dir: PathBuf,
    pub db: Connection,
}

impl State {
    pub fn new() -> Result<Self> {
        let user_dirs = UserDirs::new().context("User dirs cannot be found")?;
        let picture_dir = user_dirs
            .picture_dir()
            .context("Pictures dir needs to exist")?
            .join(PKG_NAME);
        fs::create_dir_all(&picture_dir).into_diagnostic()?;

        let project_dirs =
            ProjectDirs::from("com", "", PKG_NAME).context("Could not resolve project dirs")?;

        let data_dir = project_dirs.data_local_dir().to_path_buf();
        fs::create_dir_all(&data_dir).into_diagnostic()?;

        let conn = Connection::open(data_dir.join("kura.db")).into_diagnostic()?;

        Ok(Self {
            picture_dir,
            data_dir: data_dir.to_path_buf(),
            db: conn,
        })
    }

    pub fn prune(self) -> Result<()> {
        self.db.close().map_err(|(_, e)| miette!(e))?;

        fs::remove_dir_all(self.picture_dir).into_diagnostic()?;
        fs::remove_dir_all(self.data_dir).into_diagnostic()?;

        Ok(())
    }
}
