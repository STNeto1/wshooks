use jsonwebtoken::{DecodingKey, EncodingKey};
use once_cell::sync::Lazy;

static KEYS: Lazy<Keys> = Lazy::new(|| {
    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    Keys::new(secret.as_bytes())
});

struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Keys {
    fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

pub mod claims {
    use axum::{
        async_trait, extract::FromRequestParts, http::request::Parts, RequestPartsExt, TypedHeader,
    };
    use headers::{authorization::Bearer, Authorization};
    use jsonwebtoken::{decode, Validation};
    use serde::{Deserialize, Serialize};

    use crate::errors::CoreError;

    use super::KEYS;

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
}

pub mod http {
    use std::sync::Arc;

    use axum::{extract::State, Json};
    use jsonwebtoken::{encode, Header};
    use serde::{Deserialize, Serialize};
    use validator::Validate;

    use crate::{errors::CoreError, AppState};

    use super::{claims::Claims, KEYS};

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
        State(_state): State<Arc<AppState>>,
        Json(payload): Json<LoginPayload>,
    ) -> Result<Json<AuthBody>, CoreError> {
        match payload.validate() {
            Ok(_) => (),
            Err(e) => return Err(CoreError::BadValidation(e)),
        }

        if payload.email != "mail@mail.com" || payload.password != "102030" {
            return Err(CoreError::BadRequest(Some("Bad credentials".to_owned())));
        }

        let claims = Claims {
            sub: "mail@mail.com".to_owned(),
            exp: 2000000000, // May 2033
        };

        let token = encode(&Header::default(), &claims, &KEYS.encoding)
            .map_err(|_| CoreError::BadRequest(None))?;

        return Ok(Json(AuthBody::new(token)));
    }

    pub async fn profile(
        State(_state): State<Arc<AppState>>,
        claims: Claims,
    ) -> Result<String, CoreError> {
        dbg!(claims);
        return Err(CoreError::Unauthorized(Some(String::from("dbg!"))));
    }
}
