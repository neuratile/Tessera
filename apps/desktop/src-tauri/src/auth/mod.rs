//! Authentication primitives: password hashing, JWTs, and IPC-oriented
//! verification helpers.

pub mod dto;
pub mod jwt;
pub mod middleware;
pub mod password;

pub use dto::{canonical_email, validate_login, validate_register, LoginDto, RegisterDto};
pub use jwt::{
    decode_access_token, decode_refresh_token, encode_access_token, encode_refresh_token, Claims,
};
pub use middleware::{strip_bearer, verify_bearer_access};
pub use password::{hash_password, verify_password};
