//! Request observability (§86).
//!
//! A `tower_http::TraceLayer` that emits **one line per request** with
//! `method`, `path`, `status` and `latency_ms` — and never records any header.
//! By deliberately omitting headers (rather than trying to redact only the known
//! sensitive ones) we guarantee `cookie`, `authorization`, `x-csrf-token` and any
//! future sensitive header stay out of the diagnostic log. That leaves the
//! DB-backed `audit` trail as the only place operational facts are persisted.
//!
//! This is the *diagnostic* half of §86; the separate audit-event table is
//! unchanged. The `TraceLayer` is assembled in `http.rs` (so the concrete type
//! is inferred); this module supplies the span + response hooks.

use axum::extract::Request;
use axum::response::Response;
use std::time::Duration;
use tracing::{info_span, Span};

/// Span factory: method + path only. `uri.path()` never includes the query
/// string (so no `lat`/`lon`/`q` leak) and path params stay as literal values.
#[derive(Clone, Copy)]
pub struct RequestSpan;

impl<B> tower_http::trace::MakeSpan<B> for RequestSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        info_span!(
            "http_request",
            method = %request.method(),
            path = %request.uri().path(),
        )
    }
}

/// OnResponse: log status + latency into the request span. Headers are never
/// touched, so no cookie/token/PII can reach the log.
#[derive(Clone, Copy)]
pub struct RequestLog;

impl<B> tower_http::trace::OnResponse<B> for RequestLog {
    fn on_response(self, response: &Response<B>, latency: Duration, span: &Span) {
        let status = response.status().as_u16();
        span.in_scope(|| {
            tracing::info!(
                status = status,
                latency_ms = latency.as_millis(),
                "request completed"
            );
        });
    }
}
