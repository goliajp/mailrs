//! OIDC provider (server-side): discovery, JWKS, authorization-code
//! flow with PKCE, token endpoint (auth_code + refresh grants),
//! userinfo. Each endpoint lives in its own submodule; shared helpers
//! live alongside the handler that uses them most.

mod authorize;
mod discovery;
mod token;
mod userinfo;

pub(crate) use authorize::authorize;
pub(crate) use discovery::{jwks, openid_configuration};
pub(crate) use token::token;
pub(crate) use userinfo::userinfo;
