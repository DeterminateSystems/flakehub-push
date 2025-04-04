use color_eyre::eyre::{eyre, WrapErr};
use std::path::Path;

#[derive(Clone)]
pub(crate) struct RevisionInfo {
    pub(crate) commit_count: Option<usize>,
    pub(crate) revision: String,
}

impl RevisionInfo {
    pub(crate) fn from_git_root(git_root: &Path) -> color_eyre::Result<Self> {
        let gix_repository = gix::open(git_root).wrap_err("Opening the Git repository")?;
        let gix_repository_head = gix_repository
            .head()
            .wrap_err("Getting the HEAD revision of the repository")?;

        let revision = match gix_repository_head.kind {
            gix::head::Kind::Symbolic(gix::refs::Reference {
                name: _, target, ..
            }) => match target {
                gix::refs::Target::Object(object_id) => object_id,
                gix::refs::Target::Symbolic(_) => {
                    return Err(eyre!(
                "Symbolic revision pointing to a symbolic revision is not supported at this time"
            ))
                }
            },
            gix::head::Kind::Detached {
                target: object_id, ..
            } => object_id,
            gix::head::Kind::Unborn(_) => {
                return Err(eyre!(
                    "Newly initialized repository detected, at least one commit is necessary"
                ))
            }
        };

        let commit_count = gix_repository
            .rev_walk([revision])
            .all()
            .map(|rev_iter| rev_iter.count())
            .ok();
        let revision = revision.to_hex().to_string();

        Ok(Self {
            commit_count,
            revision,
        })
    }
}
