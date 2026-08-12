//! **What Exchange observes of every catalogued operation's configuration surface** (X-152).
//!
//! This is the *independent pre-swap check* C-534 asks for, and independence is the word doing the
//! work. Upstream's whole-catalogue differential gate compares its own two derivations — the emitted
//! Flux and the canonical document — against each other, for all 835 operations. Two derivations
//! that agree can still both differ from what the consumer was actually reading, and the consumer
//! here is [`declared_settings`] and [`operation_settings`]: the pair the connections surface, the
//! effective catalogue and the connection plan all answer *"what does this tenant still have to
//! supply"* from.
//!
//! So this file records what **this** crate reports, from this crate's public API, for every
//! provider and every operation the catalogue carries, into a committed golden. It is written
//! against the Flux-parsing derivation and must pass **unchanged** across the swap to the
//! document-backed one — the test body names neither `Rehearsal` nor `DocumentRehearsal`, which is
//! what makes "unchanged" a property of the file rather than a promise about it.
//!
//! # What a failure here means
//!
//! Not "the golden is stale". A diff means the configuration surface Exchange derives moved, and
//! every entry in it is a value an operator is asked for or is not: a setting that disappears is a
//! connector that silently stops asking, and a setting that appears is a connection that starts
//! refusing. Regenerate only after deciding the new answer is the right one — see
//! [`the_configuration_surface_is_what_the_golden_records`] for how.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use exchange_host::{declared_settings, operation_settings, DeclaredSetting};

/// The committed expected output, relative to this crate's root.
const GOLDEN: &str = "tests/golden/catalogue-configuration-surface.txt";

// ---------------------------------------------------------------------------------------------
// The surface, rendered
// ---------------------------------------------------------------------------------------------

/// One setting, as `service:binds` — the service that asks for it, its kind and its name.
///
/// `binds` is deliberately the rendering rather than the three fields separately: it is the exact
/// string `connector-pack`'s own refusal names, the string a console posts back, and therefore the
/// one whose drift is an operator-visible event.
fn render(setting: &DeclaredSetting) -> String {
    format!("{}:{}", setting.service, setting.binds())
}

/// A settings list, or `-` for a connector that asks a tenant for nothing.
fn render_all(settings: &[DeclaredSetting]) -> String {
    if settings.is_empty() {
        return "-".to_owned();
    }
    settings.iter().map(render).collect::<Vec<_>>().join(" ")
}

/// **Everything this crate reports about the catalogue's configuration surface**, deterministically.
///
/// Two records per line so a failure is a readable diff rather than a wall of JSON:
///
/// - `provider <id> <settings>` — [`declared_settings`], the whole-connector answer a connections
///   page renders;
/// - `operation <id> <provider>/<service> <settings>` — [`operation_settings`], the per-operation
///   answer effective discovery narrows with.
///
/// A refusal is recorded as `UNREADABLE`, naming the connector and operation it refuses on and
/// **not** the message: the message is a derivation's own wording, and pinning it would make this
/// golden a test of error prose rather than of the surface. Which operation refuses is the fact
/// that matters, and it is derivation-independent.
fn surface() -> String {
    let mut rendered = String::new();
    let providers = connector_catalog::providers();
    let operations: usize = providers
        .iter()
        .map(|provider| provider.operations.len())
        .sum();

    writeln!(
        rendered,
        "# Exchange's connector configuration surface, as this crate derives it (X-152)."
    )
    .expect("writing to a string does not fail");
    writeln!(
        rendered,
        "# providers={} operations={operations}",
        providers.len()
    )
    .expect("writing to a string does not fail");

    for provider in providers {
        let declared = match declared_settings(provider) {
            Ok(settings) => render_all(&settings),
            Err(refusal) => format!("UNREADABLE({})", refusal_site(&refusal)),
        };
        writeln!(rendered, "provider {} {declared}", provider.id)
            .expect("writing to a string does not fail");

        for operation in provider.operations {
            let settings = match operation_settings(provider, operation) {
                Ok(settings) => render_all(&settings),
                Err(refusal) => format!("UNREADABLE({})", refusal_site(&refusal)),
            };
            writeln!(
                rendered,
                "operation {} {}/{} {settings}",
                operation.id, provider.id, operation.service
            )
            .expect("writing to a string does not fail");
        }
    }

    rendered
}

/// The connector and operation a refusal names, without its message.
fn refusal_site(refusal: &exchange_host::SettingsRefusal) -> String {
    match refusal {
        exchange_host::SettingsRefusal::Unreadable {
            connector,
            operation,
            ..
        } => format!("{connector}/{operation}"),
        other => format!("unexpected:{other}"),
    }
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN)
}

// ---------------------------------------------------------------------------------------------
// The characterization
// ---------------------------------------------------------------------------------------------

/// **The whole surface, against the committed record.**
///
/// A missing golden is written and then *failed* rather than silently accepted: a golden that
/// generates itself on first run records whatever the code did today, which is the one thing a
/// characterization test must not do. Deleting the file and re-running is how a deliberate
/// regeneration is done, and the failure says so.
#[test]
fn the_configuration_surface_is_what_the_golden_records() {
    let path = golden_path();
    let derived = surface();

    let Ok(expected) = fs::read_to_string(&path) else {
        fs::create_dir_all(path.parent().expect("the golden has a directory"))
            .expect("the golden directory is writable");
        fs::write(&path, &derived).expect("the golden is writable");
        panic!(
            "`{GOLDEN}` did not exist and has been written from this run. Read the diff before \
             committing it: every line is a value an operator is asked for or is not."
        );
    };

    if expected == derived {
        return;
    }

    let mismatch = expected
        .lines()
        .zip(derived.lines())
        .find(|(was, now)| was != now)
        .map(|(was, now)| format!("\n  was: {was}\n  now: {now}"))
        .unwrap_or_else(|| {
            format!(
                "\n  the record has {} lines and this run derived {}",
                expected.lines().count(),
                derived.lines().count()
            )
        });

    panic!(
        "the configuration surface this crate derives is not the one `{GOLDEN}` records.{mismatch}\
         \n\nThis is not a stale golden until you have decided it is. A setting that disappeared is \
         a connector that stopped asking a tenant for a value its operations still substitute; a \
         setting that appeared is a connection that starts refusing until somebody supplies it. \
         Delete the file and re-run to regenerate, and put the diff in the commit message."
    );
}

/// **Every operation is accounted for**, so a golden that silently shrank is not a pass.
///
/// The check above compares text, and text with nothing in it compares equal to text with nothing
/// in it. This one holds the record to the catalogue's own count — 55 providers, one `operation`
/// line each — so a derivation that started refusing every connector, or a walk that stopped
/// walking, fails as a count rather than as an absence.
#[test]
fn the_golden_covers_every_catalogued_operation() {
    let expected = fs::read_to_string(golden_path()).unwrap_or_else(|error| {
        panic!("`{GOLDEN}` is committed and readable: {error}");
    });

    let providers = connector_catalog::providers();
    let operations: usize = providers
        .iter()
        .map(|provider| provider.operations.len())
        .sum();

    assert_eq!(
        expected
            .lines()
            .filter(|line| line.starts_with("provider "))
            .count(),
        providers.len(),
        "the record does not carry one line per catalogued provider",
    );
    assert_eq!(
        expected
            .lines()
            .filter(|line| line.starts_with("operation "))
            .count(),
        operations,
        "the record does not carry one line per catalogued operation",
    );
    assert!(
        !expected
            .lines()
            .any(|line| line.contains("UNREADABLE") || line.contains("unexpected:")),
        "an operation this crate cannot derive a configuration surface for is recorded in the \
         golden; that is a connector whose whole settings page is a refusal, and it should be \
         understood rather than committed",
    );
}
