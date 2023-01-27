use std::sync::Arc;

use axum::extract::Path;
use axum::{extract::State, Json};
use nanoid::nanoid;
use serde::Deserialize;
use validator::Validate;

use crate::auth::{claims::Claims, http::profile_from_claims};
use crate::{
    errors::CoreError,
    prisma::{
        resource::{self, Data},
        user,
    },
    AppState,
};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateResourcePayload {
    #[validate(length(min = 4))]
    name: String,
    key: Option<String>,
}

pub async fn create_resource(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<CreateResourcePayload>,
) -> Result<Json<Data>, CoreError> {
    match payload.validate() {
        Ok(_) => (),
        Err(e) => return Err(CoreError::BadValidation(e)),
    }

    let user = profile_from_claims(&state, claims).await?;

    let key = payload.key.unwrap_or(nanoid!());

    let existing_resource = state
        .client
        .resource()
        .find_unique(resource::key::equals(key.to_owned()))
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    match existing_resource {
        Some(_) => {
            return Err(CoreError::BadRequest(Some(
                "Resource key already exists".to_owned(),
            )));
        }
        None => (),
    }

    let new_resource = state
        .client
        .resource()
        .create(
            payload.name,
            key.to_owned(),
            user::id::equals(user.id.to_owned()),
            vec![],
        )
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;

    return Ok(Json(new_resource));
}

pub async fn list_user_resources(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> Result<Json<Vec<Data>>, CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    let resources = state
        .client
        .resource()
        .find_many(vec![resource::user::is(vec![user::id::equals(
            user.id.to_owned(),
        )])])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;

    return Ok(Json(resources));
}

pub async fn show_user_resource(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<Data>, CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    let resource = state
        .client
        .resource()
        .find_first(vec![
            resource::id::equals(id),
            resource::user::is(vec![user::id::equals(user.id.to_owned())]),
        ])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    if resource.is_none() {
        return Err(CoreError::NotFound(Some(
            "Resource was not found".to_owned(),
        )));
    }

    return Ok(Json(resource.unwrap()));
}

pub async fn delete_user_resource(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<(), CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    let resource = state
        .client
        .resource()
        .find_first(vec![
            resource::id::equals(id),
            resource::user::is(vec![user::id::equals(user.id.to_owned())]),
        ])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    if resource.is_none() {
        return Err(CoreError::NotFound(Some(
            "Resource was not found".to_owned(),
        )));
    }

    state
        .client
        .resource()
        .delete(resource::id::equals(resource.unwrap().id))
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(Some("Error deleting resource".to_owned())))?;

    return Ok(());
}
