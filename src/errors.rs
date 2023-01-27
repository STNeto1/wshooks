use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;
use validator::{ValidationErrors, ValidationErrorsKind};

pub enum CoreError {
    BadRequest(Option<String>),
    BadValidation(ValidationErrors),
    Unauthorized(Option<String>),
}

#[derive(Serialize)]
struct FieldError {
    field: String,
    field_errors: Vec<String>,
}

impl IntoResponse for CoreError {
    fn into_response(self) -> Response {
        let (status, error_message, messages) = match self {
            CoreError::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                message.unwrap_or(String::from("Bad Request")),
                None,
            ),
            CoreError::BadValidation(err) => (
                StatusCode::BAD_REQUEST,
                String::from("Bad Request"),
                Some(parse_error_messages(err)),
            ),
            CoreError::Unauthorized(message) => (
                StatusCode::UNAUTHORIZED,
                message.unwrap_or(String::from("Unauthorized")),
                None,
            ),
        };

        let body = Json(json!({
            "error": error_message,
            "messages": messages.unwrap_or(vec![])
        }));

        return (status, body).into_response();
    }
}

fn parse_error_messages(data: ValidationErrors) -> Vec<FieldError> {
    return data
        .errors()
        .iter()
        .map(|error_kind| FieldError {
            field: error_kind.0.to_string(),
            field_errors: match error_kind.1 {
                ValidationErrorsKind::Struct(struct_err) => validation_errs_to_str_vec(struct_err),
                ValidationErrorsKind::List(vec_errs) => vec_errs
                    .iter()
                    .map(|ve| {
                        format!(
                            "{}: {:?}",
                            ve.0,
                            validation_errs_to_str_vec(ve.1).join(" | "),
                        )
                    })
                    .collect(),
                ValidationErrorsKind::Field(field_errs) => field_errs
                    .iter()
                    .map(|fe| {
                        let idk = fe.message.as_ref();
                        if idk.is_none() {
                            return String::from("some bad message");
                            // TODO better message
                        }

                        return idk.unwrap().to_string();
                    })
                    .collect(),
            },
        })
        .collect();
}

fn validation_errs_to_str_vec(ve: &ValidationErrors) -> Vec<String> {
    ve.field_errors()
        .iter()
        .map(|fe| {
            format!(
                "{}: errors: {}",
                fe.0,
                fe.1.iter()
                    .map(|ve| format!("{}: {:?}", ve.code, ve.params))
                    .collect::<Vec<String>>()
                    .join(", ")
            )
        })
        .collect()
}
