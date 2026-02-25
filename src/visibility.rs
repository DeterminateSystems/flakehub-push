// Visibility has a back-compat "Hidden" variant that renames to Unlisted... Unfortunately, we need
// to allow this module-wide because the specific decorator doesn't seem to actually work
#![expect(unreachable_patterns)]

use std::fmt::Display;

#[derive(Debug, Clone, Copy, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Visibility {
    Public,
    Unlisted,
    // a backwards-compatible alias to unlisted
    #[serde(rename = "unlisted")]
    Hidden,
    Private,
}

impl Display for Visibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Visibility::Public => f.write_str("public"),
            Visibility::Hidden | Visibility::Unlisted => f.write_str("unlisted"),
            Visibility::Private => f.write_str("private"),
        }
    }
}
