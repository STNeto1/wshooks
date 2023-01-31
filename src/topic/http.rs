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
        topic::{self, Data},
        user,
    },
    AppState,
};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateTopicPayload {
    #[validate(length(min = 4))]
    name: String,
    key: Option<String>,
}

pub async fn create_topic(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Json(payload): Json<CreateTopicPayload>,
) -> Result<Json<Data>, CoreError> {
    match payload.validate() {
        Ok(_) => (),
        Err(e) => return Err(CoreError::BadValidation(e)),
    }

    let user = profile_from_claims(&state, claims).await?;

    let key = payload.key.unwrap_or(nanoid!());

    let existing_topic = state
        .client
        .topic()
        .find_unique(topic::key::equals(key.to_owned()))
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    match existing_topic {
        Some(_) => {
            return Err(CoreError::BadRequest(Some(
                "Topic key already exists".to_owned(),
            )));
        }
        None => (),
    }

    let new_topic = state
        .client
        .topic()
        .create(
            payload.name,
            key.to_owned(),
            user::id::equals(user.id.to_owned()),
            vec![],
        )
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;

    return Ok(Json(new_topic));
}

pub async fn list_user_topics(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> Result<Json<Vec<Data>>, CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    let topics = state
        .client
        .topic()
        .find_many(vec![topic::user::is(vec![user::id::equals(
            user.id.to_owned(),
        )])])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;

    return Ok(Json(topics));
}

pub async fn show_user_topic(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<Json<Data>, CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    let topic = state
        .client
        .topic()
        .find_first(vec![
            topic::id::equals(id),
            topic::user::is(vec![user::id::equals(user.id.to_owned())]),
        ])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    if topic.is_none() {
        return Err(CoreError::NotFound(Some("Topic was not found".to_owned())));
    }

    return Ok(Json(topic.unwrap()));
}

pub async fn delete_user_topic(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<(), CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    let topic = state
        .client
        .topic()
        .find_first(vec![
            topic::id::equals(id),
            topic::user::is(vec![user::id::equals(user.id.to_owned())]),
        ])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    if topic.is_none() {
        return Err(CoreError::NotFound(Some("Topic was not found".to_owned())));
    }

    state
        .client
        .topic()
        .delete(topic::id::equals(topic.unwrap().id))
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(Some("Error deleting topic".to_owned())))?;

    return Ok(());
}
