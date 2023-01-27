use std::sync::Arc;

use argon2::Config;
use axum::{extract::State, Json};
use jsonwebtoken::{encode, Header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    AppState,
    errors::CoreError,
    prisma::user::{self, Data},
};
use crate::auth::claims::Claims;
use crate::auth::KEYS;

#[derive(Debug, Serialize)]
pub struct AuthBody {
    access_token: String,
    token_type: String,
}

impl AuthBody {
    fn new(access_token: String) -> Self {
        Self {
            access_token,
            token_type: "Bearer".to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginPayload {
    #[validate(email(message = "Invalid Email"))]
    email: String,
    #[validate(length(min = 4))]
    password: String,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginPayload>,
) -> Result<Json<AuthBody>, CoreError> {
    match payload.validate() {
        Ok(_) => (),
        Err(e) => return Err(CoreError::BadValidation(e)),
    }

    let user_opt = state
        .client
        .user()
        .find_first(vec![user::email::equals(payload.email.to_owned())])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    if user_opt.is_none() {
        return Err(CoreError::BadRequest(Some(
            "Invalid credentials".to_owned(),
        )));
    }

    let usr = user_opt.unwrap();

    match argon2::verify_encoded(usr.password.as_str(), payload.password.as_bytes()) {
        Ok(_) => (),
        Err(_) => {
            return Err(CoreError::BadRequest(Some(
                "Invalid credentials".to_owned(),
            )));
        }
    };

    let claims = Claims {
        sub: usr.id.to_owned(),
        exp: 2000000000, // May 2033
    };

    let token = encode(&Header::default(), &claims, &KEYS.encoding)
        .map_err(|_| CoreError::BadRequest(None))?;

    return Ok(Json(AuthBody::new(token)));
}

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterPayload {
    #[validate(length(min = 4))]
    name: String,
    #[validate(email(message = "Invalid Email"))]
    email: String,
    #[validate(length(min = 4))]
    password: String,
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterPayload>,
) -> Result<Json<AuthBody>, CoreError> {
    match payload.validate() {
        Ok(_) => (),
        Err(e) => return Err(CoreError::BadValidation(e)),
    }

    let existing_user = state
        .client
        .user()
        .find_first(vec![user::email::equals(payload.email.to_owned())])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    match existing_user {
        Some(_) => {
            return Err(CoreError::BadRequest(Some(
                "Email already in use".to_owned(),
            )));
        }
        None => (),
    }

    let salt = b"some salt";
    let hash_result =
        argon2::hash_encoded(payload.password.as_bytes(), salt, &Config::default());
    if hash_result.is_err() {
        return Err(CoreError::InternalServerError(None));
    }

    let new_user = state
        .client
        .user()
        .create(
            Uuid::new_v4().to_string(),
            payload.name,
            payload.email,
            hash_result.unwrap(),
            vec![],
        )
        .exec()
        .await;
    if new_user.is_err() {
        println!("failed to create user");
        return Err(CoreError::InternalServerError(None));
    }

    let claims = Claims {
        sub: new_user.unwrap().id.to_owned(),
        exp: 2000000000, // May 2033
    };

    let token = encode(&Header::default(), &claims, &KEYS.encoding);
    if token.is_err() {
        println!("failed to create token");
        return Err(CoreError::InternalServerError(None));
    }

    return Ok(Json(AuthBody::new(token.unwrap())));
}

#[derive(Debug, Serialize)]
pub struct Profile {
    id: String,
    name: String,
    email: String,
}

pub async fn profile(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> Result<Json<Profile>, CoreError> {
    let user = profile_from_claims(&state, claims).await?;

    return Ok(Json(Profile {
        id: user.id,
        name: user.name,
        email: user.email,
    }));
}

pub async fn profile_from_claims(
    state: &Arc<AppState>,
    claims: Claims,
) -> Result<Data, CoreError> {
    let subject = state
        .client
        .user()
        .find_first(vec![user::id::equals(claims.sub)])
        .exec()
        .await
        .map_err(|_| CoreError::InternalServerError(None))?;
    if subject.is_none() {
        return Err(CoreError::Unauthorized(None));
    }

    return Ok(subject.unwrap());
}
