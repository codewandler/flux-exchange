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

const MAX_DIAGNOSTIC_BYTES: usize = 4096;
const BOUNDED_DIAGNOSTIC: &str =
    "invocation refused; the provider diagnostic exceeded the exchange.api.v1 bound";

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
        let message = refusal.to_string();
        // A diagnostic is not request authority. Replacing an over-bound diagnostic with a fixed
        // refusal preserves the outcome while preventing an upstream error from widening the wire.
        let message = if message.len() <= MAX_DIAGNOSTIC_BYTES {
            message
        } else {
            BOUNDED_DIAGNOSTIC.to_owned()
        };
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
    Refusal(InvocationRefusalBody),
}

#[cfg(test)]
pub(crate) fn decode_invoke_response(status: u16, bytes: &[u8]) -> Result<InvokeResponse, String> {
    reject_duplicate_members(bytes)?;
    match status {
        200 => serde_json::from_slice::<Invocation>(bytes)
            .map(InvokeResponse::Success)
            .map_err(|error| error.to_string()),
        403 | 404 | 409 | 422 | 502 => {
            let body: InvocationRefusalBody =
                serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            if !body.admits_status(status) {
                return Err("invocation refusal status and body disagree".to_owned());
            }
            Ok(InvokeResponse::Refusal(body))
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
        InvokeResponse::Refusal(refusal) => serde_json::to_vec(refusal),
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
    }
    if contains_authority_axis(&value) {
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
fn credential_shaped(value: &str) -> bool {
    [
        "Bearer ",
        "password=",
        "token=",
        "secret=",
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
        expected: String,
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
            assert!(!file.expected.is_empty(), "{} has no outcome", file.name);
            let bytes = std::fs::read(directory.join(&file.name)).expect("declared fixture");
            let digest = Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(digest, file.sha256, "{}", file.name);
        }
        assert_eq!(
            actual, declared,
            "every fixture must be covered by the inventory"
        );
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
