//! Startup configuration for the authentication-acquisition hazards this deployment permits.
//!
//! Policy lives in [`exchange_host::AuthPosture`]; this module is only the binary composition's
//! by-name environment binding. It is deliberately not reachable from a request handler. A caller
//! may name a connector and supply acquisition material later, but it may never select the policy
//! against which that acquisition is decided.

use std::fmt;

use exchange_host::{AuthHazard, AuthPosture};

/// The comma-separated authentication hazards this deployment knowingly permits.
pub const ALLOW_AUTH_HAZARDS_ENV: &str = "FLUX_EXCHANGE_ALLOW_AUTH_HAZARDS";

/// Read the deployment's posture from its environment.
pub fn configured() -> Result<AuthPosture, AuthPostureConfigRefusal> {
    read(|name| std::env::var(name).ok())
}

/// Read by name from an injected source, so tests do not mutate process-wide environment.
fn read(
    lookup: impl FnOnce(&str) -> Option<String>,
) -> Result<AuthPosture, AuthPostureConfigRefusal> {
    let Some(raw) = lookup(ALLOW_AUTH_HAZARDS_ENV) else {
        return Ok(AuthPosture::fail_closed());
    };

    let mut allowed = Vec::new();
    for named in raw
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let hazard = match named {
            known if known == AuthHazard::ResourceOwnerSecretShared.as_str() => {
                AuthHazard::ResourceOwnerSecretShared
            }
            unknown => {
                return Err(AuthPostureConfigRefusal::UnknownHazard {
                    value: unknown.to_owned(),
                });
            }
        };
        allowed.push(hazard);
    }

    Ok(AuthPosture::allowing(allowed))
}

/// Why deployment authentication policy could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPostureConfigRefusal {
    /// The allow-list named a hazard this build cannot decide on.
    UnknownHazard {
        /// The unrecognised, non-secret policy spelling.
        value: String,
    },
}

impl fmt::Display for AuthPostureConfigRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHazard { value } => write!(
                f,
                "refusing to start: {ALLOW_AUTH_HAZARDS_ENV} names unrecognised authentication \
                 hazard `{value}`; this build recognises `{}`",
                AuthHazard::ResourceOwnerSecretShared
            ),
        }
    }
}

impl std::error::Error for AuthPostureConfigRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_is_read_by_name_and_unset_is_fail_closed() {
        let mut read_name = None;
        let posture = read(|name| {
            read_name = Some(name.to_owned());
            None
        })
        .expect("unset is a valid, fail-closed posture");

        assert_eq!(read_name.as_deref(), Some(ALLOW_AUTH_HAZARDS_ENV));
        assert!(posture
            .admit("fixture", AuthHazard::ResourceOwnerSecretShared)
            .is_err());
    }

    #[test]
    fn an_unknown_value_refuses_the_whole_setting_and_names_it() {
        let refusal = read(|name| {
            assert_eq!(name, ALLOW_AUTH_HAZARDS_ENV);
            Some("resource_owner_secret_shared,newly_invented_hazard".to_owned())
        })
        .expect_err("an unknown remainder must not be skipped");
        let message = refusal.to_string();

        assert!(matches!(
            refusal,
            AuthPostureConfigRefusal::UnknownHazard { ref value }
                if value == "newly_invented_hazard"
        ));
        assert!(message.contains(ALLOW_AUTH_HAZARDS_ENV), "{message}");
        assert!(message.contains("newly_invented_hazard"), "{message}");
    }

    #[test]
    fn a_recognised_value_arms_only_that_typed_hazard() {
        let posture = read(|_| Some(" resource_owner_secret_shared ".to_owned()))
            .expect("the documented value is recognised");

        posture
            .admit("fixture", AuthHazard::ResourceOwnerSecretShared)
            .expect("the explicitly named hazard is armed");
    }
}
