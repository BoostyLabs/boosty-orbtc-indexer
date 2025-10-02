use actix::fut::{ready, Ready};
use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::web::Data;
use actix_web::{Error, FromRequest};
use serde::{Deserialize, Serialize};

const AUTH_HEADER: &str = "x-api-key";
use super::context::Context;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct XApiKey(pub String);

impl FromRequest for XApiKey {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        let key_result = req.headers().get(AUTH_HEADER).and_then(|v| v.to_str().ok());

        let v = match key_result {
            Some(key) => Ok(XApiKey(key.to_owned())),
            // NOTE: if api key is not present,
            // then it will populate Option<XApiKey> with None,
            // which is what we want.
            None => Err(api_core::api_errors::access_denied().into()),
        };

        ready(v)
    }
}

pub async fn ensure_api_key(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let (request, _) = req.parts();

    if request.uri().path().ends_with("/status") {
        // invoke the wrapped middleware or service
        return next.call(req).await;
    }

    let Some(token) = request
        .headers()
        .get(AUTH_HEADER)
        .and_then(|h| h.to_str().ok())
    else {
        return Err(api_core::api_errors::access_denied().into());
    };

    // token is present in the request cookies
    let state = req
        .app_data::<Data<Context>>()
        .expect("Context should be present")
        .clone();

    let Some(api_key) = state.get_api_key(token) else {
        return Err(api_core::api_errors::access_denied().into());
    };
    if api_key.blocked {
        return Err(api_core::api_errors::forbidden().into());
    }

    // invoke the wrapped middleware or service
    next.call(req).await
}
