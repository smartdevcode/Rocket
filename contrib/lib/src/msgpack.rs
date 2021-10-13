//! Automatic MessagePack (de)serialization support.
//!
//! See the [`MsgPack`](crate::msgpack::MsgPack) type for further details.
//!
//! # Enabling
//!
//! This module is only available when the `msgpack` feature is enabled. Enable
//! it in `Cargo.toml` as follows:
//!
//! ```toml
//! [dependencies.rocket_contrib]
//! version = "0.5.0-dev"
//! default-features = false
//! features = ["msgpack"]
//! ```

use std::ops::{Deref, DerefMut};

use futures::io::AsyncReadExt;

use rocket::request::Request;
use rocket::outcome::Outcome::*;
use rocket::data::{Data, FromData, FromDataFuture, Transform::*, TransformFuture, Transformed};
use rocket::http::Status;
use rocket::response::{self, content, Responder};
use rocket::AsyncReadExt as _;

use serde::Serialize;
use serde::de::Deserialize;

pub use rmp_serde::decode::Error;

/// The `MsgPack` type: implements [`FromData`] and [`Responder`], allowing you
/// to easily consume and respond with MessagePack data.
///
/// ## Receiving MessagePack
///
/// If you're receiving MessagePack data, simply add a `data` parameter to your
/// route arguments and ensure the type of the parameter is a `MsgPack<T>`,
/// where `T` is some type you'd like to parse from MessagePack. `T` must
/// implement [`Deserialize`] from [`serde`]. The data is parsed from the HTTP
/// request body.
///
/// ```rust
/// # #![feature(proc_macro_hygiene)]
/// # #[macro_use] extern crate rocket;
/// # extern crate rocket_contrib;
/// # type User = usize;
/// use rocket_contrib::msgpack::MsgPack;
///
/// #[post("/users", format = "msgpack", data = "<user>")]
/// fn new_user(user: MsgPack<User>) {
///     /* ... */
/// }
/// ```
///
/// You don't _need_ to use `format = "msgpack"`, but it _may_ be what you want.
/// Using `format = msgpack` means that any request that doesn't specify
/// "application/msgpack" as its first `Content-Type:` header parameter will not
/// be routed to this handler.
///
/// ## Sending MessagePack
///
/// If you're responding with MessagePack data, return a `MsgPack<T>` type,
/// where `T` implements [`Serialize`] from [`serde`]. The content type of the
/// response is set to `application/msgpack` automatically.
///
/// ```rust
/// # #![feature(proc_macro_hygiene)]
/// # #[macro_use] extern crate rocket;
/// # extern crate rocket_contrib;
/// # type User = usize;
/// use rocket_contrib::msgpack::MsgPack;
///
/// #[get("/users/<id>")]
/// fn user(id: usize) -> MsgPack<User> {
///     let user_from_id = User::from(id);
///     /* ... */
///     MsgPack(user_from_id)
/// }
/// ```
///
/// ## Incoming Data Limits
///
/// The default size limit for incoming MessagePack data is 1MiB. Setting a
/// limit protects your application from denial of service (DOS) attacks and
/// from resource exhaustion through high memory consumption. The limit can be
/// increased by setting the `limits.msgpack` configuration parameter. For
/// instance, to increase the MessagePack limit to 5MiB for all environments,
/// you may add the following to your `Rocket.toml`:
///
/// ```toml
/// [global.limits]
/// msgpack = 5242880
/// ```
#[derive(Debug)]
pub struct MsgPack<T>(pub T);

impl<T> MsgPack<T> {
    /// Consumes the `MsgPack` wrapper and returns the wrapped item.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use rocket_contrib::msgpack::MsgPack;
    /// let string = "Hello".to_string();
    /// let my_msgpack = MsgPack(string);
    /// assert_eq!(my_msgpack.into_inner(), "Hello".to_string());
    /// ```
    #[inline(always)]
    pub fn into_inner(self) -> T {
        self.0
    }
}

/// Default limit for MessagePack is 1MB.
const LIMIT: u64 = 1 << 20;

impl<'a, T: Deserialize<'a>> FromData<'a> for MsgPack<T> {
    type Error = Error;
    type Owned = Vec<u8>;
    type Borrowed = [u8];

    fn transform(r: &Request<'_>, d: Data) -> TransformFuture<'a, Self::Owned, Self::Error> {
        let size_limit = r.limits().get("msgpack").unwrap_or(LIMIT);

        Box::pin(async move {
            let mut buf = Vec::new();
            let mut reader = d.open().take(size_limit);
            match reader.read_to_end(&mut buf).await {
                Ok(_) => Borrowed(Success(buf)),
                Err(e) => Borrowed(Failure((Status::BadRequest, Error::InvalidDataRead(e)))),
            }
        })
    }

    fn from_data(_: &Request<'_>, o: Transformed<'a, Self>) -> FromDataFuture<'a, Self, Self::Error> {
        use self::Error::*;

        Box::pin(async move {
            let buf = try_outcome!(o.borrowed());
            match rmp_serde::from_slice(&buf) {
                Ok(val) => Success(MsgPack(val)),
                Err(e) => {
                    error_!("Couldn't parse MessagePack body: {:?}", e);
                    match e {
                        TypeMismatch(_) | OutOfRange | LengthMismatch(_) => {
                            Failure((Status::UnprocessableEntity, e))
                        }
                        _ => Failure((Status::BadRequest, e)),
                    }
                }
            }
        })
    }
}

/// Serializes the wrapped value into MessagePack. Returns a response with
/// Content-Type `MsgPack` and a fixed-size body with the serialization. If
/// serialization fails, an `Err` of `Status::InternalServerError` is returned.
impl<'r, T: Serialize> Responder<'r> for MsgPack<T> {
    fn respond_to(self, req: &'r Request<'_>) -> response::ResultFuture<'r> {
        match rmp_serde::to_vec(&self.0) {
            Ok(buf) => content::MsgPack(buf).respond_to(req),
            Err(e) => Box::pin(async move {
                error_!("MsgPack failed to serialize: {:?}", e);
                Err(Status::InternalServerError)
            }),
        }
    }
}

impl<T> Deref for MsgPack<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for MsgPack<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}
