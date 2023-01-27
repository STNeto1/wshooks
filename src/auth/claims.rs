use axum::{
    async_trait, extract::FromRequestParts, http::request::Parts, RequestPartsExt, TypedHeader,
};
use headers::{Authorization, authorization::Bearer};
use jsonwebtoken::{decode, Validation};
use serde::{Deserialize, Serialize};

use crate::{auth::KEYS, errors::CoreError};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

// handle the injection into the handler
#[async_trait]
impl<S> FromRequestParts<S> for Claims
    where
        S: Send + Sync,
{
    type Rejection = CoreError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        // Extract the token from the authorization header
        let TypedHeader(Authorization(bearer)) = parts
            .extract::<TypedHeader<Authorization<Bearer>>>()
            .await
            .map_err(|_| CoreError::Unauthorized(None))?;
        // Decode the user data
        let token_data =
            decode::<Claims>(bearer.token(), &KEYS.decoding, &Validation::default())
                .map_err(|_| CoreError::Unauthorized(None))?;

        Ok(token_data.claims)
    }
}
