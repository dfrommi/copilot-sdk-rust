// Copyright (c) 2026 Elias Bachaalany
// SPDX-License-Identifier: MIT

//! Built-in W3C Trace Context propagation for OpenTelemetry.
//!
//! When the `opentelemetry` feature is enabled, [`inject_trace_context`] is
//! called before each outgoing `session.create`, `session.resume`, and
//! `session.send` JSON-RPC request. It reads `traceparent` / `tracestate`
//! from the current [`opentelemetry::Context`] using a local
//! [`TraceContextPropagator`] and merges them into the params object so the
//! CLI's spans join the same distributed trace.
//!
//! A local propagator is used on purpose: the application does **not** need
//! to call `opentelemetry::global::set_text_map_propagator(...)` — the
//! default global propagator is a no-op and would silently drop the context.
//!
//! If no OpenTelemetry context is active (no `TracerProvider` set up, or no
//! span on the stack), this is a silent no-op — no errors, no panics.

use opentelemetry::propagation::{Injector, TextMapPropagator};
use opentelemetry::Context;
use opentelemetry_sdk::propagation::TraceContextPropagator;

/// Inject W3C `traceparent` / `tracestate` from the current OpenTelemetry
/// context into a JSON-RPC params object.
///
/// Does nothing if the context carries no span or if `params` is not a JSON
/// object.
pub(crate) fn inject_trace_context(params: &mut serde_json::Value) {
    let serde_json::Value::Object(map) = params else {
        return;
    };
    TraceContextPropagator::new().inject_context(&Context::current(), &mut JsonMapInjector(map));
}

struct JsonMapInjector<'a>(&'a mut serde_json::Map<String, serde_json::Value>);

impl Injector for JsonMapInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if key == "traceparent" || key == "tracestate" {
            self.0
                .insert(key.to_string(), serde_json::Value::String(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{
        SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState,
    };

    #[test]
    fn inject_without_active_span_is_noop() {
        let mut params = serde_json::json!({ "sessionId": "s1" });
        inject_trace_context(&mut params);
        assert_eq!(params, serde_json::json!({ "sessionId": "s1" }));
    }

    #[test]
    fn inject_into_non_object_is_noop() {
        let mut params = serde_json::json!("not an object");
        inject_trace_context(&mut params);
        assert_eq!(params, serde_json::json!("not an object"));
    }

    #[test]
    fn inject_with_active_span_writes_traceparent() {
        // Synthetic remote span context attached to the current OTel context.
        // The local TraceContextPropagator should produce a well-formed W3C
        // traceparent without any global propagator being registered.
        let trace_id = TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap();
        let span_id = SpanId::from_hex("00f067aa0ba902b7").unwrap();
        let span_ctx = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            true,
            TraceState::default(),
        );
        let cx = Context::current().with_remote_span_context(span_ctx);
        let _guard = cx.attach();

        let mut params = serde_json::json!({ "sessionId": "s1" });
        inject_trace_context(&mut params);
        assert_eq!(params["sessionId"], "s1");
        assert_eq!(
            params["traceparent"],
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
    }
}
