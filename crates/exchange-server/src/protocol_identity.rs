//! Provider-owned protocol identities shared by every local-release surface.

use std::fmt;

use serde::Serialize;

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
