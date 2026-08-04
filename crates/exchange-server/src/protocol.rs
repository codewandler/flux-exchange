//! Provider-owned identities and strict conformance for the delivered Exchange HTTP v1 wire.
//!
//! These are capability identities, not package versions. The release channel, manifest,
//! compatibility document and readiness record all serialize these constants; none is permitted
//! to derive a protocol id from `CARGO_PKG_VERSION`.

use std::fmt;

#[cfg(test)]
use exchange_host::Invocation;
use exchange_host::{InvokeRefusal, Sent};
use serde::{Deserialize, Serialize};

/// A protocol identity whose spelling is fixed by its provider contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ProtocolId(&'static str);

impl ProtocolId {
    const fn new(value: &'static str) -> Self {
        Self(value)
    }

    /// The exact compatibility value serialized on the wire.
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ProtocolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Service Account bearer authentication and the two delivered Milestone 1 routes.
pub const EXCHANGE_API_V1: ProtocolId = ProtocolId::new("exchange.api.v1");
/// The strict `EffectiveCatalogue` response contract.
pub const EFFECTIVE_CATALOGUE_RESPONSE_V1: ProtocolId =
    ProtocolId::new("exchange.effective-catalogue-response.v1");
/// The raw operation body and optional sole `connection` query contract.
pub const INVOKE_REQUEST_V1: ProtocolId = ProtocolId::new("exchange.invoke-request.v1");
/// The invocation success and closed refusal response contract.
pub const INVOKE_RESPONSE_V1: ProtocolId = ProtocolId::new("exchange.invoke-response.v1");
/// The closed declaration-driven connection plan and submission contract.
pub const CONNECTION_PLAN_V1: ProtocolId = ProtocolId::new("exchange.connection-plan.v1");
/// The inherited-capability ABI and one-shot supervised readiness contract.
pub const SUPERVISOR_READY_V1: ProtocolId = ProtocolId::new("exchange.supervisor-ready.v1");

/// The complete protocol identity advertised by every local-release surface.
///
/// Keeping this as one serializable value prevents the release channel, manifest, compatibility
/// command and readiness writer from independently spelling a capability or deriving one from the
/// package version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProtocolVersions {
    /// The declaration-driven connection plan contract.
    pub connection_plan: ProtocolId,
    /// The authenticated Exchange HTTP surface.
    pub exchange_api: ProtocolId,
    /// The effective catalogue response.
    pub effective_catalogue_response: ProtocolId,
    /// The raw invocation request.
    pub invoke_request: ProtocolId,
    /// The closed invocation response.
    pub invoke_response: ProtocolId,
    /// The supervised inherited-capability and readiness contract.
    pub supervisor: ProtocolId,
}

/// The six exact provider-owned protocol versions supported by this executable.
pub const PROTOCOL_VERSIONS: ProtocolVersions = ProtocolVersions {
    connection_plan: CONNECTION_PLAN_V1,
    exchange_api: EXCHANGE_API_V1,
    effective_catalogue_response: EFFECTIVE_CATALOGUE_RESPONSE_V1,
    invoke_request: INVOKE_REQUEST_V1,
    invoke_response: INVOKE_RESPONSE_V1,
    supervisor: SUPERVISOR_READY_V1,
};

const MAX_DIAGNOSTIC_BYTES: usize = 4096;
const BOUNDED_DIAGNOSTIC: &str =
    "request refused; diagnostic omitted by the exchange.invoke-response.v1 bound";

fn bounded_diagnostic(error: impl Into<String>) -> String {
    let error = error.into();
    if error.len() <= MAX_DIAGNOSTIC_BYTES && !credential_shaped(&error) {
        error
    } else {
        BOUNDED_DIAGNOSTIC.to_owned()
    }
}

/// The production refusal shared by authentication, malformed input, limits and unavailable ports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorBody {
    error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowConnectionRefusalBody {
    refusal: WorkflowConnectionRefusalCode,
    message: String,
}

impl WorkflowConnectionRefusalBody {
    pub(crate) fn invalid_connection_selector() -> Self {
        Self {
            refusal: WorkflowConnectionRefusalCode::InvalidConnectionSelector,
            message: "a stored workflow is not one connector connection; remove `connection`"
                .to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkflowConnectionRefusalCode {
    InvalidConnectionSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowRunRefusalBody {
    error: String,
    run: String,
}

impl WorkflowRunRefusalBody {
    pub(crate) fn new(error: impl Into<String>, run: impl Into<String>) -> Self {
        Self {
            error: bounded_diagnostic(error),
            run: run.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkflowLookupRefusalBody {
    error: String,
    current_revision: Option<u64>,
}

impl WorkflowLookupRefusalBody {
    pub(crate) fn new(error: impl Into<String>, current_revision: Option<u64>) -> Self {
        Self {
            error: bounded_diagnostic(error),
            current_revision,
        }
    }
}

impl ErrorBody {
    pub(crate) fn new(error: impl Into<String>) -> Self {
        Self {
            error: bounded_diagnostic(error),
        }
    }

    #[cfg(test)]
    fn is_bounded_and_value_free(&self) -> bool {
        self.error.len() <= MAX_DIAGNOSTIC_BYTES && !credential_shaped(&self.error)
    }
}

/// One closed machine-readable invocation refusal on the v1 route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InvocationRefusalBody {
    refusal: InvocationRefusalCode,
    operation: Option<String>,
    sent: Sent,
    retryable: bool,
    message: String,
    supply_at: Option<String>,
}

impl InvocationRefusalBody {
    pub(crate) fn from_refusal(refusal: &InvokeRefusal, supply_at: Option<String>) -> Self {
        // A diagnostic is not request authority. Replacing an over-bound or credential-shaped
        // diagnostic with a fixed refusal preserves the outcome without widening the wire.
        let message = bounded_diagnostic(refusal.to_string());
        Self {
            refusal: InvocationRefusalCode::from(refusal),
            operation: refusal.operation().map(str::to_owned),
            sent: refusal.sent(),
            retryable: refusal.retryable(),
            message,
            supply_at,
        }
    }

    #[cfg(test)]
    fn admits_status(&self, status: u16) -> bool {
        status == self.refusal.status()
            && match self.refusal {
                InvocationRefusalCode::UnknownOperation
                | InvocationRefusalCode::RuntimeRefused
                | InvocationRefusalCode::NotGranted
                | InvocationRefusalCode::Refused => self.sent == Sent::No && !self.retryable,
                InvocationRefusalCode::Transport => self.sent == Sent::Maybe,
            }
            && match self.refusal {
                InvocationRefusalCode::RuntimeRefused => self.operation.is_none(),
                _ => self.operation.is_some(),
            }
            && match self.refusal {
                InvocationRefusalCode::Refused => self.supply_at.is_some(),
                _ => self.supply_at.is_none(),
            }
            && self.message.len() <= MAX_DIAGNOSTIC_BYTES
            && !credential_shaped(&self.message)
            && self
                .operation
                .as_deref()
                .is_none_or(|value| !credential_shaped(value))
            && self
                .supply_at
                .as_deref()
                .is_none_or(|value| !credential_shaped(value))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum InvocationRefusalCode {
    UnknownOperation,
    RuntimeRefused,
    NotGranted,
    Refused,
    Transport,
}

impl InvocationRefusalCode {
    #[cfg(test)]
    const fn status(self) -> u16 {
        match self {
            Self::UnknownOperation => 404,
            Self::RuntimeRefused => 409,
            Self::NotGranted => 403,
            Self::Refused => 422,
            Self::Transport => 502,
        }
    }
}

impl From<&InvokeRefusal> for InvocationRefusalCode {
    fn from(refusal: &InvokeRefusal) -> Self {
        match refusal {
            InvokeRefusal::UnknownOperation { .. } => Self::UnknownOperation,
            InvokeRefusal::Runtime(_) => Self::RuntimeRefused,
            InvokeRefusal::NotGranted { .. } => Self::NotGranted,
            InvokeRefusal::Refused { .. } => Self::Refused,
            InvokeRefusal::Transport { .. } => Self::Transport,
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvokeResponse {
    Success(Invocation),
    WorkflowSuccess(exchange_host::WorkflowInvocation),
    Refusal(InvocationRefusalBody),
    Error(ErrorBody),
    Connection(ConnectionSelectionRefusal),
    WorkflowConnection(WorkflowConnectionRefusalBody),
    WorkflowRun(WorkflowRunRefusalBody),
    WorkflowLookup(WorkflowLookupRefusalBody),
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConnectionSelectionRefusal {
    UnknownLabel(UnknownLabelRefusal),
    Coded(CodedConnectionRefusal),
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnknownLabelRefusal {
    connector: String,
    label: String,
    error: String,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodedConnectionRefusal {
    connector: String,
    code: ConnectionSelectionCode,
    error: String,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectionSelectionCode {
    Disconnected,
    AmbiguousConnection,
}

#[cfg(test)]
impl ConnectionSelectionRefusal {
    fn admits_status(&self, status: u16) -> bool {
        match self {
            Self::UnknownLabel(body) => {
                status == 404
                    && !body.label.is_empty()
                    && !credential_shaped(&body.connector)
                    && !credential_shaped(&body.label)
                    && body.error.len() <= MAX_DIAGNOSTIC_BYTES
                    && !credential_shaped(&body.error)
            }
            Self::Coded(body) => {
                status == 409
                    && !credential_shaped(&body.connector)
                    && body.error.len() <= MAX_DIAGNOSTIC_BYTES
                    && !credential_shaped(&body.error)
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn decode_invoke_response(status: u16, bytes: &[u8]) -> Result<InvokeResponse, String> {
    reject_duplicate_members(bytes)?;
    match status {
        200 => {
            if let Ok(invocation) = serde_json::from_slice::<Invocation>(bytes) {
                if credential_shaped(&invocation.operation)
                    || credential_shaped(&invocation.content)
                    || invocation.view.as_deref().is_some_and(credential_shaped)
                {
                    return Err("invocation success contains a credential-shaped value".to_owned());
                }
                return Ok(InvokeResponse::Success(invocation));
            }
            let invocation: exchange_host::WorkflowInvocation =
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            if credential_shaped(&invocation.operation)
                || credential_shaped(&invocation.workflow_id)
                || credential_shaped(&invocation.content)
            {
                return Err("workflow success contains a credential-shaped value".to_owned());
            }
            Ok(InvokeResponse::WorkflowSuccess(invocation))
        }
        403 | 404 | 409 | 422 | 502 => {
            if let Ok(body) = serde_json::from_slice::<InvocationRefusalBody>(bytes) {
                if !body.admits_status(status) {
                    return Err("invocation refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::Refusal(body));
            }
            if let Ok(body) = serde_json::from_slice::<ConnectionSelectionRefusal>(bytes) {
                if !body.admits_status(status) {
                    return Err("connection-selection refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::Connection(body));
            }
            if let Ok(body) = serde_json::from_slice::<WorkflowConnectionRefusalBody>(bytes) {
                if status != 422
                    || body.message.len() > MAX_DIAGNOSTIC_BYTES
                    || credential_shaped(&body.message)
                {
                    return Err("workflow connection refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::WorkflowConnection(body));
            }
            if let Ok(body) = serde_json::from_slice::<WorkflowRunRefusalBody>(bytes) {
                if !matches!(status, 403 | 409 | 422 | 502)
                    || body.run.is_empty()
                    || body.run.len() > 128
                    || body.error.len() > MAX_DIAGNOSTIC_BYTES
                    || credential_shaped(&body.error)
                    || credential_shaped(&body.run)
                {
                    return Err("workflow run refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::WorkflowRun(body));
            }
            if let Ok(body) = serde_json::from_slice::<ErrorBody>(bytes) {
                if !matches!(status, 422 | 502) || !body.is_bounded_and_value_free() {
                    return Err("generic invocation refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::Error(body));
            }
            if let Ok(body) = serde_json::from_slice::<WorkflowLookupRefusalBody>(bytes) {
                if !matches!(status, 404 | 409 | 422)
                    || body.error.len() > MAX_DIAGNOSTIC_BYTES
                    || credential_shaped(&body.error)
                {
                    return Err("workflow lookup refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::WorkflowLookup(body));
            }
            Err("response body does not match a closed invocation refusal".to_owned())
        }
        400 | 401 | 429 | 503 => {
            if let Ok(body) = serde_json::from_slice::<ErrorBody>(bytes) {
                if !body.is_bounded_and_value_free() {
                    return Err("invocation error body exceeds its value-free bound".to_owned());
                }
                return Ok(InvokeResponse::Error(body));
            }
            if let Ok(body) = serde_json::from_slice::<WorkflowLookupRefusalBody>(bytes) {
                if status != 503
                    || body.error.len() > MAX_DIAGNOSTIC_BYTES
                    || credential_shaped(&body.error)
                {
                    return Err("workflow lookup refusal status and body disagree".to_owned());
                }
                return Ok(InvokeResponse::WorkflowLookup(body));
            }
            Err("response body does not match a closed invocation refusal".to_owned())
        }
        _ => Err(format!(
            "status {status} is not an exchange.invoke-response.v1 outcome"
        )),
    }
}

#[cfg(test)]
fn encode_invoke_response(response: &InvokeResponse) -> Result<Vec<u8>, serde_json::Error> {
    match response {
        InvokeResponse::Success(invocation) => serde_json::to_vec(invocation),
        InvokeResponse::WorkflowSuccess(invocation) => serde_json::to_vec(invocation),
        InvokeResponse::Refusal(refusal) => serde_json::to_vec(refusal),
        InvokeResponse::Error(error) => serde_json::to_vec(error),
        InvokeResponse::Connection(connection) => serde_json::to_vec(connection),
        InvokeResponse::WorkflowConnection(refusal) => serde_json::to_vec(refusal),
        InvokeResponse::WorkflowRun(refusal) => serde_json::to_vec(refusal),
        InvokeResponse::WorkflowLookup(refusal) => serde_json::to_vec(refusal),
    }
}

#[cfg(test)]
pub(crate) fn decode_effective_catalogue(
    bytes: &[u8],
) -> Result<crate::routes::catalogue::view::EffectiveCatalogue, String> {
    use std::collections::BTreeSet;

    let value = reject_duplicate_members(bytes)?;
    let object = value
        .as_object()
        .ok_or_else(|| "effective catalogue must be an object".to_owned())?;
    let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if keys != BTreeSet::from(["generation", "operations"]) {
        return Err("effective catalogue has missing or unknown fields".to_owned());
    }
    let generation = object["generation"]
        .as_str()
        .filter(|generation| {
            generation.len() == 71
                && generation.starts_with("sha256:")
                && generation[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| {
            "effective catalogue generation is not a stable SHA-256 identity".to_owned()
        })?;
    let _ = generation;
    let operations = object["operations"]
        .as_array()
        .ok_or_else(|| "effective catalogue operations must be an array".to_owned())?;
    let expected = BTreeSet::from([
        "admitted",
        "connection",
        "description",
        "effects",
        "effects_derived",
        "id",
        "idempotency",
        "input_schema",
        "risk",
        "service",
    ]);
    for operation in operations {
        let object = operation
            .as_object()
            .ok_or_else(|| "effective operation must be an object".to_owned())?;
        let keys = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
        if keys != expected {
            return Err("effective operation has missing or unknown fields".to_owned());
        }
        if object["admitted"] != serde_json::Value::Bool(true)
            || !(object["connection"].is_null() || object["connection"].is_string())
        {
            return Err("effective operation null/omission rules changed".to_owned());
        }
        let schema = object["input_schema"]
            .as_object()
            .ok_or_else(|| "effective operation input_schema must be an object".to_owned())?;
        let schema_keys = schema.keys().map(String::as_str).collect::<BTreeSet<_>>();
        let properties = schema["properties"].as_object();
        let required = schema["required"].as_array();
        if schema_keys != BTreeSet::from(["properties", "required", "type"])
            || schema["type"] != serde_json::Value::String("object".to_owned())
            || properties.is_none()
            || !required.is_some_and(|required| {
                let names = required.iter().filter_map(serde_json::Value::as_str);
                let names = names.collect::<BTreeSet<_>>();
                names.len() == required.len()
                    && properties.is_some_and(|properties| {
                        names.iter().all(|name| properties.contains_key(*name))
                    })
            })
            || !properties.is_some_and(|properties| {
                properties
                    .values()
                    .all(|property| property.as_object().is_some_and(|value| !value.is_empty()))
            })
        {
            return Err("effective operation input_schema object contract changed".to_owned());
        }
    }
    if contains_authority_axis(&value) || contains_sensitive_value(&value) {
        return Err("effective catalogue contains a caller authority axis".to_owned());
    }
    let decoded: crate::routes::catalogue::view::EffectiveCatalogue =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    let regenerated =
        crate::routes::catalogue::view::EffectiveCatalogue::new(decoded.operations.clone())
            .map_err(|error| error.to_string())?;
    if decoded.generation != regenerated.generation {
        return Err("effective catalogue generation does not identify its operations".to_owned());
    }
    Ok(decoded)
}

#[cfg(test)]
fn contains_authority_axis(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(values) => values.iter().any(contains_authority_axis),
        serde_json::Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(
                key.as_str(),
                "tenant" | "authority" | "credential" | "endpoint" | "host" | "runtime" | "uuid"
            ) || contains_authority_axis(value)
        }),
        _ => false,
    }
}

#[cfg(test)]
fn contains_sensitive_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(value) => credential_shaped(value),
        serde_json::Value::Array(values) => values.iter().any(contains_sensitive_value),
        serde_json::Value::Object(values) => values.values().any(contains_sensitive_value),
        _ => false,
    }
}

fn credential_shaped(value: &str) -> bool {
    [
        "Bearer ",
        "password=",
        "token=",
        "secret=",
        "credential://",
        "https://attacker.example",
        "/tenants/attacker/",
        "quiggle-marrow-plimth-42",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}

#[cfg(test)]
fn reject_duplicate_members(bytes: &[u8]) -> Result<serde_json::Value, String> {
    use serde::de::{MapAccess, SeqAccess, Visitor};

    struct Unique(serde_json::Value);
    impl<'de> Deserialize<'de> for Unique {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct UniqueVisitor;
            impl<'de> Visitor<'de> for UniqueVisitor {
                type Value = Unique;

                fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str("JSON with unique object members")
                }

                fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                    Ok(Unique(value.into()))
                }

                fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                    Ok(Unique(value.into()))
                }

                fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                    Ok(Unique(value.into()))
                }

                fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                    Ok(Unique(value.into()))
                }

                fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
                where
                    E: serde::de::Error,
                {
                    Ok(Unique(value.into()))
                }

                fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                    Ok(Unique(value.into()))
                }

                fn visit_none<E>(self) -> Result<Self::Value, E> {
                    Ok(Unique(serde_json::Value::Null))
                }

                fn visit_unit<E>(self) -> Result<Self::Value, E> {
                    Ok(Unique(serde_json::Value::Null))
                }

                fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    let mut values = Vec::new();
                    while let Some(Unique(value)) = sequence.next_element()? {
                        values.push(value);
                    }
                    Ok(Unique(values.into()))
                }

                fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                where
                    A: MapAccess<'de>,
                {
                    let mut values = serde_json::Map::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if values.contains_key(&key) {
                            return Err(serde::de::Error::custom(format!(
                                "duplicate JSON member `{key}`"
                            )));
                        }
                        let Unique(value) = map.next_value()?;
                        values.insert(key, value);
                    }
                    Ok(Unique(values.into()))
                }
            }
            deserializer.deserialize_any(UniqueVisitor)
        }
    }

    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let Unique(value) =
        Unique::deserialize(&mut deserializer).map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::catalogue::view::{effective_operation, EffectiveCatalogue};
    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/exchange-http-v1")
    }

    #[test]
    fn the_four_http_protocol_ids_are_typed_and_exact() {
        assert_eq!(EXCHANGE_API_V1.as_str(), "exchange.api.v1");
        assert_eq!(
            EFFECTIVE_CATALOGUE_RESPONSE_V1.as_str(),
            "exchange.effective-catalogue-response.v1"
        );
        assert_eq!(INVOKE_REQUEST_V1.as_str(), "exchange.invoke-request.v1");
        assert_eq!(INVOKE_RESPONSE_V1.as_str(), "exchange.invoke-response.v1");
    }

    #[test]
    fn production_success_types_refuse_shape_drift() {
        let invocation =
            br#"{"operation":"github-repo-get","content":"{}","view":null,"is_error":false}"#;
        let decoded = decode_invoke_response(200, invocation).expect("the v1 success shape");
        assert_eq!(
            encode_invoke_response(&decoded).expect("encode"),
            invocation
        );

        for changed in [
            br#"{"operation":"github-repo-get","content":"{}","view":null,"is_error":false,"tenant":"acme"}"#.as_slice(),
            br#"{"operation":"github-repo-get","content":"{}","view":null}"#.as_slice(),
            br#"{"operation":"github-repo-get","content":"{}","view":null,"is_error":false,"is_error":false}"#.as_slice(),
        ] {
            assert!(decode_invoke_response(200, changed).is_err());
        }
        for field in ["operation", "content", "view"] {
            let mut changed: serde_json::Value =
                serde_json::from_slice(invocation).expect("success fixture");
            changed[field] = serde_json::json!("quiggle-marrow-plimth-42");
            assert!(decode_invoke_response(200, changed.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn effective_catalogue_round_trips_the_production_type_and_generation() {
        let operation =
            connector_catalog::operation(connector_catalog::OperationKey::id("github-repo-get"))
                .expect("released read operation");
        let catalogue = EffectiveCatalogue::new(vec![effective_operation(
            operation,
            Some("prod".to_owned()),
        )
        .expect("projection")])
        .expect("generation");
        let bytes = serde_json::to_vec(&catalogue).expect("production serializer");
        assert_eq!(
            decode_effective_catalogue(&bytes).expect("strict production decoder"),
            catalogue
        );

        let mut changed: serde_json::Value = serde_json::from_slice(&bytes).expect("fixture value");
        changed["tenant"] = serde_json::json!("acme");
        assert!(
            decode_effective_catalogue(&serde_json::to_vec(&changed).expect("mutation")).is_err()
        );
        let mut changed: serde_json::Value = serde_json::from_slice(&bytes).expect("fixture value");
        changed["generation"] = serde_json::json!(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert!(
            decode_effective_catalogue(&serde_json::to_vec(&changed).expect("mutation")).is_err()
        );

        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("fixture value");
        let mut mutations = Vec::new();
        let mut changed = value.clone();
        changed
            .as_object_mut()
            .expect("object")
            .remove("generation");
        mutations.push(changed);
        let mut changed = value.clone();
        changed["operations"][0]
            .as_object_mut()
            .expect("operation")
            .remove("description");
        mutations.push(changed);
        let mut changed = value.clone();
        changed["operations"][0]["connection"] = serde_json::json!(false);
        mutations.push(changed);
        let mut changed = value.clone();
        changed["operations"][0]["input_schema"] = serde_json::Value::Null;
        mutations.push(changed);
        let mut changed = value.clone();
        changed["operations"][0]["input_schema"]["required"] = serde_json::json!(null);
        mutations.push(changed);
        let mut changed = value.clone();
        changed["operations"][0]["description"] = serde_json::json!("quiggle-marrow-plimth-42");
        mutations.push(changed);
        let mut changed = value;
        changed["operations"][0]["input_schema"]["endpoint"] =
            serde_json::json!("https://attacker.example");
        mutations.push(changed);
        for changed in mutations {
            assert!(
                decode_effective_catalogue(&serde_json::to_vec(&changed).expect("mutation"))
                    .is_err()
            );
        }
        let duplicate = bytes
            .strip_suffix(b"}")
            .expect("object ending")
            .iter()
            .copied()
            .chain(br#","generation":"sha256:580047103e8e73a1f94f95a746817dea2218dcec0f817630953e836c2f3cdb66"}"#.iter().copied())
            .collect::<Vec<_>>();
        assert!(decode_effective_catalogue(&duplicate).is_err());
    }

    #[test]
    fn refusal_status_and_body_are_one_closed_contract() {
        let accepted = br#"{"refusal":"transport","operation":"github-repo-get","sent":"maybe","retryable":true,"message":"transport unavailable","supply_at":null}"#;
        let decoded = decode_invoke_response(502, accepted).expect("transport refusal");
        assert_eq!(encode_invoke_response(&decoded).expect("encode"), accepted);

        for (status, changed) in [
            (403, accepted.as_slice()),
            (502, br#"{"refusal":"transport","operation":"github-repo-get","sent":"no","retryable":true,"message":"transport unavailable","supply_at":null}"#.as_slice()),
            (502, br#"{"refusal":"transport","operation":"github-repo-get","sent":"maybe","retryable":true,"message":"Bearer secret","supply_at":null}"#.as_slice()),
            (502, br#"{"refusal":"transport","operation":"github-repo-get","sent":"maybe","retryable":true,"message":"transport unavailable","supply_at":null,"endpoint":"https://attacker"}"#.as_slice()),
        ] {
            assert!(decode_invoke_response(status, changed).is_err());
        }
        let unbounded = serde_json::json!({
            "refusal": "transport",
            "operation": "github-repo-get",
            "sent": "maybe",
            "retryable": true,
            "message": "x".repeat(MAX_DIAGNOSTIC_BYTES + 1),
            "supply_at": null,
        });
        assert!(decode_invoke_response(502, unbounded.to_string().as_bytes()).is_err());
        for field in ["operation", "message", "supply_at"] {
            let mut changed: serde_json::Value =
                serde_json::from_slice(accepted).expect("refusal fixture");
            changed[field] = serde_json::json!("quiggle-marrow-plimth-42");
            assert!(decode_invoke_response(502, changed.to_string().as_bytes()).is_err());
        }
    }

    #[test]
    fn every_production_error_constructor_applies_one_value_free_diagnostic_bound() {
        for diagnostic in [
            "x".repeat(MAX_DIAGNOSTIC_BYTES + 1),
            "quiggle-marrow-plimth-42".to_owned(),
        ] {
            let bodies = [
                (
                    503,
                    serde_json::to_vec(&ErrorBody::new(&diagnostic)).expect("generic error"),
                ),
                (
                    403,
                    serde_json::to_vec(&WorkflowRunRefusalBody::new(
                        &diagnostic,
                        "01J00000000000000000000000",
                    ))
                    .expect("workflow run error"),
                ),
                (
                    404,
                    serde_json::to_vec(&WorkflowLookupRefusalBody::new(&diagnostic, None))
                        .expect("workflow lookup error"),
                ),
            ];
            for (status, bytes) in bodies {
                assert!(!String::from_utf8_lossy(&bytes).contains(&diagnostic));
                let decoded = decode_invoke_response(status, &bytes)
                    .expect("production bounded body satisfies its decoder");
                assert_eq!(encode_invoke_response(&decoded).expect("encode"), bytes);
            }
        }

        for (status, raw) in [
            (
                403,
                serde_json::json!({
                    "error": "x".repeat(MAX_DIAGNOSTIC_BYTES + 1),
                    "run": "01J00000000000000000000000",
                }),
            ),
            (
                404,
                serde_json::json!({
                    "error": "x".repeat(MAX_DIAGNOSTIC_BYTES + 1),
                    "current_revision": null,
                }),
            ),
        ] {
            assert!(decode_invoke_response(status, raw.to_string().as_bytes()).is_err());
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Inventory {
        schema: String,
        files: Vec<InventoryFile>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct InventoryFile {
        name: String,
        sha256: String,
        expected: ExpectedOutcome,
    }

    #[derive(Debug, Clone, Copy, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum ExpectedOutcome {
        #[serde(rename = "authentication_401")]
        Authentication401,
        #[serde(rename = "connection_selection_404")]
        ConnectionSelection404,
        #[serde(rename = "connection_selection_409")]
        ConnectionSelection409,
        EffectiveCatalogueAccepted,
        RetryableTransportRefusalAccepted,
        InvocationSuccessAccepted,
        #[serde(rename = "invalid_connection_label_422")]
        InvalidConnectionLabel422,
        #[serde(rename = "malformed_request_400")]
        MalformedRequest400,
        #[serde(rename = "no_invoker_503")]
        NoInvoker503,
        #[serde(rename = "registry_unavailable_503")]
        RegistryUnavailable503,
        RequestCaseOutcomesChecked,
        #[serde(rename = "store_denied_502")]
        StoreDenied502,
        #[serde(rename = "store_unavailable_503")]
        StoreUnavailable503,
        #[serde(rename = "traffic_429")]
        Traffic429,
        #[serde(rename = "workflow_connection_422")]
        WorkflowConnection422,
        #[serde(rename = "workflow_lookup_404")]
        WorkflowLookup404,
        #[serde(rename = "workflow_run_refusal_403")]
        WorkflowRunRefusal403,
        WorkflowSuccessAccepted,
    }

    impl ExpectedOutcome {
        fn validate(self, bytes: &[u8]) {
            let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
            match self {
                Self::EffectiveCatalogueAccepted => {
                    let decoded =
                        decode_effective_catalogue(bytes).expect("effective outcome accepts");
                    assert_eq!(
                        serde_json::to_vec(&decoded).expect("production serializer"),
                        bytes
                    );
                }
                Self::RequestCaseOutcomesChecked => {
                    let _: Requests = serde_json::from_slice(bytes).expect("typed request cases");
                }
                other => {
                    let status = match other {
                        Self::InvocationSuccessAccepted => 200,
                        Self::WorkflowSuccessAccepted => 200,
                        Self::MalformedRequest400 => 400,
                        Self::Authentication401 => 401,
                        Self::WorkflowRunRefusal403 => 403,
                        Self::ConnectionSelection404 => 404,
                        Self::WorkflowLookup404 => 404,
                        Self::ConnectionSelection409 => 409,
                        Self::Traffic429 => 429,
                        Self::InvalidConnectionLabel422 | Self::WorkflowConnection422 => 422,
                        Self::RetryableTransportRefusalAccepted | Self::StoreDenied502 => 502,
                        Self::NoInvoker503
                        | Self::RegistryUnavailable503
                        | Self::StoreUnavailable503 => 503,
                        Self::EffectiveCatalogueAccepted | Self::RequestCaseOutcomesChecked => {
                            unreachable!("handled above")
                        }
                    };
                    let decoded = decode_invoke_response(status, bytes)
                        .unwrap_or_else(|error| panic!("{self:?}: {error}"));
                    assert_eq!(
                        encode_invoke_response(&decoded).expect("production serializer"),
                        bytes
                    );
                }
            }
        }
    }

    #[test]
    fn provider_fixture_inventory_checks_every_filename_digest_and_outcome() {
        let directory = fixtures();
        let inventory: Inventory = serde_json::from_slice(
            &std::fs::read(directory.join("inventory.json")).expect("fixture inventory"),
        )
        .expect("typed fixture inventory");
        assert_eq!(inventory.schema, "exchange.http-fixtures.v1");
        let actual = std::fs::read_dir(&directory)
            .expect("fixture directory")
            .map(|entry| {
                entry
                    .expect("fixture entry")
                    .file_name()
                    .into_string()
                    .expect("UTF-8 fixture name")
            })
            .collect::<BTreeSet<_>>();
        let mut declared = BTreeSet::from(["inventory.json".to_owned()]);
        for file in &inventory.files {
            assert!(declared.insert(file.name.clone()), "duplicate fixture name");
            let bytes = std::fs::read(directory.join(&file.name)).expect("declared fixture");
            let digest = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(digest, file.sha256, "{}", file.name);
            file.expected.validate(&bytes);
        }
        assert_eq!(
            actual, declared,
            "every fixture must be covered by the inventory"
        );

        let mut invalid: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.join("inventory.json")).expect("fixture inventory"),
        )
        .expect("inventory value");
        invalid["files"][0]["expected"] = serde_json::json!("anything_nonempty");
        assert!(serde_json::from_value::<Inventory>(invalid).is_err());
    }

    #[test]
    fn checked_wire_fixtures_round_trip_only_through_production_types() {
        let directory = fixtures();
        let effective = std::fs::read(directory.join("effective-catalogue-response.json"))
            .expect("effective fixture");
        let effective = effective.strip_suffix(b"\n").unwrap_or(&effective);
        let decoded = decode_effective_catalogue(effective).expect("effective fixture accepted");
        assert_eq!(
            serde_json::to_vec(&decoded).expect("production serializer"),
            effective
        );

        for (name, status) in [("invoke-success.json", 200), ("invoke-refusal.json", 502)] {
            let bytes = std::fs::read(directory.join(name)).expect("invoke fixture");
            let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
            let decoded = decode_invoke_response(status, bytes).expect("invoke fixture accepted");
            assert_eq!(
                encode_invoke_response(&decoded).expect("production serializer"),
                bytes
            );
        }
    }

    #[derive(Deserialize)]
    struct Requests {
        protocols: RequestProtocols,
    }

    #[derive(Deserialize)]
    struct RequestProtocols {
        exchange_api: String,
        effective_catalogue_response: String,
        invoke_request: String,
        invoke_response: String,
    }

    #[test]
    fn fixture_protocol_values_are_derived_from_the_typed_provider_constants() {
        let requests: Requests = serde_json::from_slice(
            &std::fs::read(fixtures().join("requests.json")).expect("request fixture"),
        )
        .expect("typed request fixture");
        assert_eq!(requests.protocols.exchange_api, EXCHANGE_API_V1.as_str());
        assert_eq!(
            requests.protocols.effective_catalogue_response,
            EFFECTIVE_CATALOGUE_RESPONSE_V1.as_str()
        );
        assert_eq!(
            requests.protocols.invoke_request,
            INVOKE_REQUEST_V1.as_str()
        );
        assert_eq!(
            requests.protocols.invoke_response,
            INVOKE_RESPONSE_V1.as_str()
        );
    }
}
