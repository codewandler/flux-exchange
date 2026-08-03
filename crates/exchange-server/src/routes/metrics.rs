//! Fixed-cardinality process traffic measurements for the deployment scraper.

use axum::extract::State;
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};

use super::{Access, Module, Route};
use crate::state::AppState;

pub(super) const MODULE: Module = Module {
    name: "metrics",
    routes: &[Route {
        path: "/metrics",
        access: Access::Anonymous,
        method_router: route,
    }],
};

fn route() -> MethodRouter<AppState> {
    get(metrics)
}

async fn metrics(State(state): State<AppState>) -> Response {
    let metrics = state.traffic_snapshot();
    let body = format!(
        concat!(
            "# TYPE flux_exchange_traffic_total counter\n",
            "flux_exchange_traffic_total{{work=\"signin\",outcome=\"admitted\",limit=\"none\"}} {}\n",
            "flux_exchange_traffic_total{{work=\"signin\",outcome=\"refused\",limit=\"global\"}} {}\n",
            "flux_exchange_traffic_total{{work=\"invocation\",outcome=\"admitted\",limit=\"none\"}} {}\n",
            "flux_exchange_traffic_total{{work=\"invocation\",outcome=\"refused\",limit=\"principal\"}} {}\n",
            "flux_exchange_traffic_total{{work=\"invocation\",outcome=\"refused\",limit=\"global\"}} {}\n",
            "flux_exchange_traffic_total{{work=\"invocation\",outcome=\"refused\",limit=\"concurrency\"}} {}\n",
            "# TYPE flux_exchange_active_invocations gauge\n",
            "flux_exchange_active_invocations {}\n"
        ),
        metrics.sign_ins_admitted,
        metrics.sign_ins_refused,
        metrics.invocations_admitted,
        metrics.invocations_refused_principal,
        metrics.invocations_refused_global,
        metrics.invocations_refused_concurrency,
        metrics.active_invocations,
    );
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn metrics_have_only_the_fixed_dimensions() {
        let response = metrics(State(AppState::without_identity())).await;
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("metrics body");
        let body = String::from_utf8(body.to_vec()).expect("text metrics");

        assert!(body.contains("limit=\"principal\""));
        assert!(body.contains("flux_exchange_active_invocations 0"));
        assert!(!body.contains("tenant"));
        assert!(!body.contains("principal="));
    }
}
