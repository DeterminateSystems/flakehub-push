use tokio::io::AsyncWriteExt;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("The `GITHUB_OUTPUT` environment variable is unset.")]
    GithubOutputUnset,

    #[error("Failure opening {0:?}: {1}")]
    OpenFile(std::ffi::OsString, std::io::Error),

    #[error("Writing to {0:?}: {1}")]
    WriteFile(std::ffi::OsString, std::io::Error),

    #[error("Key contains delimiter")]
    KeyContainsDelimiter,

    #[error("Value contains delimiter")]
    ValueContainsDelimiter,
}

pub(crate) async fn set_output<'a>(name: &'a str, value: &'a str) -> Result<(), Error> {
    let output_path = std::env::var_os("GITHUB_OUTPUT").ok_or(Error::GithubOutputUnset)?;
    let mut fh = tokio::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .truncate(false)
        .open(&output_path)
        .await
        .map_err(|e| Error::OpenFile(output_path.clone(), e))?;

    fh.write_all(escape_key_value(name, value)?.as_bytes())
        .await
        .map_err(|e| Error::WriteFile(output_path, e))?;

    Ok(())
}

fn escape_key_value<'a>(key: &'a str, value: &'a str) -> Result<String, Error> {
    // see: https://github.com/actions/toolkit/blob/6dd369c0e648ed58d0ead326cf2426906ea86401/packages/core/src/file-command.ts#L27-L47
    let delimiter = format!("ghadelimiter_{}", uuid::Uuid::new_v4());
    let eol = '\n';

    if key.contains(&delimiter) {
        return Err(Error::KeyContainsDelimiter);
    }

    if value.contains(&delimiter) {
        return Err(Error::ValueContainsDelimiter);
    }

    Ok(format!(
        "{key}<<{delimiter}{eol}{value}{eol}{delimiter}{eol}"
    ))
}
