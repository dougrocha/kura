//! `GET` and `PATCH /config`: the shared config file, for the Mac app's
//! Settings. Only mounted with `--config-api`, which the app passes: a
//! `kura serve` shared over the network shouldn't let callers pick folders.

use std::path::PathBuf;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use kura_core::{ConfigError, ConfigFile};

use crate::{
    error::{ApiError, ApiResult},
    extract::Json as JsonBody,
    wire,
};

#[derive(Clone)]
pub(crate) struct ConfigState {
    pub(crate) file: ConfigFile,
    /// The images folder this server opened.
    pub(crate) pictures: PathBuf,
}

pub(crate) fn router(state: ConfigState) -> Router {
    Router::new().route("/config", get(read).patch(change)).with_state(state)
}

async fn read(State(state): State<ConfigState>) -> ApiResult<Json<wire::Config>> {
    Ok(Json(snapshot(&state)?))
}

async fn change(
    State(state): State<ConfigState>,
    JsonBody(body): JsonBody<wire::ConfigChange>,
) -> ApiResult<Json<wire::Config>> {
    match &body.value {
        Some(value) => state.file.set(&body.key, value),
        None => state.file.unset(&body.key),
    }
    .map_err(api_error)?;
    Ok(Json(snapshot(&state)?))
}

fn snapshot(state: &ConfigState) -> ApiResult<wire::Config> {
    let config = state.file.load().map_err(api_error)?;
    Ok(wire::Config {
        path: state.file.path().display().to_string(),
        library: state.file.get("library").map_err(api_error)?,
        library_folder: state.pictures.display().to_string(),
        close_after_copy: config.close_after_copy,
        relaunch_when_hidden: config.relaunch_when_hidden,
    })
}

fn api_error(err: ConfigError) -> ApiError {
    match err {
        ConfigError::Io { .. } | ConfigError::NoHome => {
            ApiError::request(StatusCode::INTERNAL_SERVER_ERROR, "config_unavailable", err.to_string())
        }
        _ => ApiError::request(StatusCode::BAD_REQUEST, "invalid_config", err.to_string()),
    }
}
