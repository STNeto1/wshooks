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
    use headers::{Authorization, authorization::Bearer};
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

    use argon2::Config;
    use axum::{extract::State, Json};
    use jsonwebtoken::{encode, Header};
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;
    use validator::Validate;

    use crate::{AppState, errors::CoreError, prisma::user};

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
            return Err(CoreError::BadRequest(Some("Invalid credentials".to_owned())));
        }

        let usr = user_opt.unwrap();

        match argon2::verify_encoded(usr.password.as_str(), payload.password.as_bytes()) {
            Ok(_) => (),
            Err(_) => return Err(CoreError::BadRequest(Some("Invalid credentials".to_owned()))),
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
            Some(_) => return Err(CoreError::BadRequest(Some("Email already in use".to_owned()))),
            None => ()
        }

        let salt = b"some salt";
        let hash_result = argon2::hash_encoded(payload.password.as_bytes(), salt, &Config::default());
        if hash_result.is_err() {
            return Err(CoreError::InternalServerError(None));
        }

        let new_user = state.client.user().create(
            Uuid::new_v4().to_string(),
            payload.name,
            payload.email,
            hash_result.unwrap(),
            vec![],
        ).exec().await;
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

        let unwrapped_user = subject.unwrap();

        return Ok(Json(Profile {
            id: unwrapped_user.id,
            name: unwrapped_user.name,
            email: unwrapped_user.email,
        }));
    }
}
