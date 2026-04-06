use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// Marker extension: was the request from loopback?
#[derive(Debug, Clone, Copy)]
pub struct IsLocalhost(pub bool);

/// Marker extension: the raw client IP string.
#[derive(Debug, Clone)]
pub struct ClientIp(pub String);

/// Permission tier assigned to a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PeerTier {
    NoAccess = 0,
    ReadOnly = 1,
    ApprovalRequired = 2,
    FullAccess = 3,
}

impl PeerTier {
    pub fn from_str(s: &str) -> Self {
        match s {
            "read-only" => PeerTier::ReadOnly,
            "approval-required" => PeerTier::ApprovalRequired,
            "full-access" => PeerTier::FullAccess,
            _ => PeerTier::NoAccess,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PeerTier::NoAccess => "no-access",
            PeerTier::ReadOnly => "read-only",
            PeerTier::ApprovalRequired => "approval-required",
            PeerTier::FullAccess => "full-access",
        }
    }
}

fn forbidden_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        axum::Json(serde_json::json!({
            "error": "forbidden",
            "message": message
        })),
    )
        .into_response()
}

/// Marker extension: the request came from an approval-required peer.
#[derive(Debug, Clone, Copy)]
pub struct NeedsApproval;

/// Extractor that passes only if the peer's tier >= FullAccess.
/// For `ApprovalRequired` peers, the request is allowed through but
/// `NeedsApproval` is inserted into extensions so write handlers can queue it.
pub struct RequireFullAccess;

impl<S> FromRequestParts<S> for RequireFullAccess
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let tier = parts
            .extensions
            .get::<PeerTier>()
            .copied()
            .unwrap_or(PeerTier::NoAccess);

        match tier {
            PeerTier::FullAccess => Ok(RequireFullAccess),
            PeerTier::ApprovalRequired => {
                parts.extensions.insert(NeedsApproval);
                Ok(RequireFullAccess)
            }
            _ => Err(forbidden_response("full-access tier required")),
        }
    }
}

/// Extractor that reports whether the current request needs approval queueing.
pub struct OptionalNeedsApproval(pub bool);

impl<S> FromRequestParts<S> for OptionalNeedsApproval
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(OptionalNeedsApproval(
            parts.extensions.get::<NeedsApproval>().is_some(),
        ))
    }
}

impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts
            .extensions
            .get::<ClientIp>()
            .cloned()
            .unwrap_or_else(|| ClientIp(String::new())))
    }
}

/// Extractor that passes only if the peer's tier >= ReadOnly.
pub struct RequireReadOnly;

pub struct CurrentPeerTier(pub PeerTier);

impl<S> FromRequestParts<S> for RequireReadOnly
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let tier = parts
            .extensions
            .get::<PeerTier>()
            .copied()
            .unwrap_or(PeerTier::NoAccess);

        if tier >= PeerTier::ReadOnly {
            Ok(RequireReadOnly)
        } else {
            Err(forbidden_response("read-only tier required"))
        }
    }
}

impl<S> FromRequestParts<S> for CurrentPeerTier
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(CurrentPeerTier(
            parts.extensions
                .get::<PeerTier>()
                .copied()
                .unwrap_or(PeerTier::NoAccess),
        ))
    }
}

/// Extractor that passes only if the request came from localhost.
pub struct RequireLocalhostOnly;

impl<S> FromRequestParts<S> for RequireLocalhostOnly
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let is_local = parts
            .extensions
            .get::<IsLocalhost>()
            .map(|il| il.0)
            .unwrap_or(false);

        if is_local {
            Ok(RequireLocalhostOnly)
        } else {
            Err(forbidden_response("localhost only"))
        }
    }
}
