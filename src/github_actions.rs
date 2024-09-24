use tokio::io::AsyncWriteExt;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("The `GITHUB_OUTPUT` environment variable is unset.")]
    GithubOutputUnset,

    #[error("Failure opening {0:?}: {1}")]
    OpenFile(std::ffi::OsString, std::io::Error),

    #[error("Writing to {0:?}: {1}")]
    WriteFile(std::ffi::OsString, std::io::Error),
}

pub(crate) async fn set_output<'a>(name: &'a str, value: &'a str) -> Result<(), Error> {
    let output_path = std::env::var_os("GITHUB_OUTPUT").ok_or(Error::GithubOutputUnset)?;
    let mut fh = tokio::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&output_path)
        .await
        .map_err(|e| Error::OpenFile(output_path.clone(), e))?;

    fh.write_all(format!("{}={}\n", name, value).as_bytes())
        .await
        .map_err(|e| Error::WriteFile(output_path, e))?;

    Ok(())
}
