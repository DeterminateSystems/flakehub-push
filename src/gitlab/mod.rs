use color_eyre::eyre::WrapErr;

#[tracing::instrument(skip_all, fields(audience = tracing::field::Empty))]
pub(crate) async fn get_runner_bearer_token() -> color_eyre::Result<String> {
    // github allows you to at-runtime change the audience of the token
    // gitlab requires job-level audience/token config, and makes it available via envvar

    let maybe_token = std::env::var("GITLAB_JWT_ID_TOKEN");
    let token = maybe_token
        .wrap_err("Failed to get a JWT from GitLab. You must configure id_token in the jobs.")?;

    // for now, don't bother validating audience, since flakehub will validate the audience as part of authn anyway

    Ok(token)
}
