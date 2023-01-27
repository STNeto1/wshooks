pub mod http {
    use std::sync::Arc;

    use axum::{extract::State, Json};
    use nanoid::nanoid;
    use serde::Deserialize;
    use validator::Validate;

    use crate::{
        AppState,
        errors::CoreError,
        prisma::{
            resource::{self, Data},
            user,
        },
    };
    use crate::auth::{claims::Claims, http::profile_from_claims};

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
}
