use std::fmt;

use jiff::civil::DateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageHash(pub String);

impl std::hash::Hash for ImageHash {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl std::ops::Deref for ImageHash {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for ImageHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub struct CreatedAt(pub DateTime);
