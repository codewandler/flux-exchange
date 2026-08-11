#[path = "../src/local_helper.rs"]
mod local_helper;

use std::ffi::OsString;
use std::time::{Duration, Instant};

use local_helper::{
    validate_complete_frame_size, validate_unix_vendor_capabilities,
    validate_windows_vendor_capabilities, CapabilityRefusal, ExpiresAt, FrameSizeRefusal,
    HelperDeadlineSchedule, HelperExit, HelperGrammarRefusal, HelperPlatform,
    LocalHelperEndpointPort, LocalHelperInvocation, MintWriterCapability, PipeCapabilityFacts,
    PipeDirection, ServiceAccountId, UnixVendorCapabilityFacts, VendorSecretCapabilities,
    WindowsVendorCapabilityFacts, HELPER_RESULT_DEADLINE, HELPER_SETUP_DEADLINE,
    MAX_HELPER_FRAME_BYTES, UNIX_MINT_WRITER_FD, UNIX_VENDOR_REQUEST_FD, UNIX_VENDOR_RESPONSE_FD,
};

#[test]
fn absolute_helper_deadlines_close_the_4_5_and_334_335_boundaries() {
    let request_eof = Instant::now();
    let request_by = request_eof + Duration::from_secs(5);
    assert!(HelperDeadlineSchedule::permits(
        request_by,
        request_eof + Duration::from_secs(4)
    ));
    assert!(!HelperDeadlineSchedule::permits(request_by, request_by));

    let schedule = HelperDeadlineSchedule::from_request_eof(request_eof).expect("deadlines");
    assert!(HelperDeadlineSchedule::permits(
        schedule.setup_by(),
        request_eof + Duration::from_secs(4)
    ));
    assert!(!HelperDeadlineSchedule::permits(
        schedule.setup_by(),
        request_eof + Duration::from_secs(5)
    ));

    assert!(HelperDeadlineSchedule::permits(
        schedule.result_by(),
        request_eof + Duration::from_secs(334)
    ));
    assert!(!HelperDeadlineSchedule::permits(
        schedule.result_by(),
        request_eof + Duration::from_secs(335)
    ));
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn parse(
    platform: HelperPlatform,
    values: &[&str],
) -> Result<LocalHelperInvocation, HelperGrammarRefusal> {
    local_helper::parse_local_helper(platform, &args(values))
}

fn assert_closed_shape(platform: HelperPlatform, accepted: &[&str]) {
    assert!(parse(platform, accepted).is_ok());

    for removed in 0..accepted.len() {
        let mut candidate = accepted.to_vec();
        candidate.remove(removed);
        assert!(parse(platform, &candidate).is_err());
    }

    for insertion in 0..=accepted.len() {
        let mut candidate = accepted.to_vec();
        candidate.insert(insertion, "extra");
        assert!(parse(platform, &candidate).is_err());
    }

    let structural = (0..accepted.len())
        .filter(|index| *index < 2 || index % 2 == 0)
        .collect::<Vec<_>>();
    for (position, left) in structural.iter().enumerate() {
        for right in structural.iter().skip(position + 1) {
            let mut candidate = accepted.to_vec();
            candidate.swap(*left, *right);
            assert!(parse(platform, &candidate).is_err());
        }
    }
}

#[test]
fn unix_vendor_grammar_is_one_exact_argument_vector() {
    assert_closed_shape(HelperPlatform::Unix, &["local", "vendor-secret"]);
    assert!(matches!(
        parse(HelperPlatform::Unix, &["local", "vendor-secret"]),
        Ok(LocalHelperInvocation::VendorSecret(
            VendorSecretCapabilities::Unix
        ))
    ));

    for rejected in [
        vec![],
        vec!["local"],
        vec!["vendor-secret"],
        vec!["local", "vendor-secret", "extra"],
        vec!["local", "--vendor-secret"],
        vec!["local", "vendor-secret", "--request-handle", "1"],
        vec!["--dev", "local", "vendor-secret"],
    ] {
        assert!(matches!(
            parse(HelperPlatform::Unix, &rejected),
            Err(HelperGrammarRefusal::Grammar)
        ));
    }
}

#[test]
fn windows_vendor_handles_are_ordered_canonical_nonzero_and_distinct() {
    let invocation = parse(
        HelperPlatform::Windows,
        &[
            "local",
            "vendor-secret",
            "--request-handle",
            "17",
            "--response-handle",
            "42",
        ],
    );
    let Ok(LocalHelperInvocation::VendorSecret(VendorSecretCapabilities::Windows {
        request,
        response,
    })) = invocation
    else {
        panic!("exact Windows grammar was refused")
    };
    assert_eq!(request.native_value(), 17);
    assert_eq!(response.native_value(), 42);

    let maximum = usize::MAX.to_string();
    let maximum_args = vec![
        OsString::from("local"),
        OsString::from("vendor-secret"),
        OsString::from("--request-handle"),
        OsString::from(maximum),
        OsString::from("--response-handle"),
        OsString::from("42"),
    ];
    assert!(local_helper::parse_local_helper(HelperPlatform::Windows, &maximum_args).is_ok());

    for malformed in ["", "0", "00", "01", "+1", "-1", " 1", "1 ", "1_0", "abc"] {
        let invocation = [
            "local",
            "vendor-secret",
            "--request-handle",
            malformed,
            "--response-handle",
            "42",
        ];
        assert!(matches!(
            parse(HelperPlatform::Windows, &invocation),
            Err(HelperGrammarRefusal::Handle)
        ));
    }

    let overflow = (usize::MAX as u128 + 1).to_string();
    let overflow_args = vec![
        OsString::from("local"),
        OsString::from("vendor-secret"),
        OsString::from("--request-handle"),
        OsString::from(overflow),
        OsString::from("--response-handle"),
        OsString::from("42"),
    ];
    assert!(matches!(
        local_helper::parse_local_helper(HelperPlatform::Windows, &overflow_args),
        Err(HelperGrammarRefusal::Handle)
    ));

    assert!(matches!(
        parse(
            HelperPlatform::Windows,
            &[
                "local",
                "vendor-secret",
                "--request-handle",
                "7",
                "--response-handle",
                "7"
            ]
        ),
        Err(HelperGrammarRefusal::DuplicateHandle)
    ));
}

#[test]
fn windows_vendor_rejects_every_missing_extra_or_reordered_shape() {
    assert_closed_shape(
        HelperPlatform::Windows,
        &[
            "local",
            "vendor-secret",
            "--request-handle",
            "1",
            "--response-handle",
            "2",
        ],
    );
    let shapes = [
        vec!["local", "vendor-secret"],
        vec![
            "local",
            "vendor-secret",
            "--response-handle",
            "2",
            "--request-handle",
            "1",
        ],
        vec![
            "local",
            "vendor-secret",
            "--request-handle",
            "1",
            "--response-handle",
        ],
        vec![
            "local",
            "vendor-secret",
            "--request-handle",
            "1",
            "--response-handle",
            "2",
            "extra",
        ],
        vec![
            "local",
            "vendor-secret",
            "--request-handle=1",
            "--response-handle=2",
        ],
    ];
    for shape in shapes {
        assert!(matches!(
            parse(HelperPlatform::Windows, &shape),
            Err(HelperGrammarRefusal::Grammar)
        ));
    }
}

#[test]
fn unix_mint_grammar_is_closed_and_retains_no_secret_value() {
    assert_closed_shape(
        HelperPlatform::Unix,
        &[
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            "1",
            "--writer-fd",
            "5",
        ],
    );
    let invocation = parse(
        HelperPlatform::Unix,
        &[
            "local",
            "service-account-mint",
            "--id",
            "release_bot-1",
            "--expires-at",
            "9223372036854775807",
            "--writer-fd",
            "5",
        ],
    );
    let Ok(LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::UnixFd5,
    }) = invocation
    else {
        panic!("exact Unix mint grammar was refused")
    };
    assert_eq!(id.as_str(), "release_bot-1");
    assert_eq!(expires_at.value(), i64::MAX);

    for replacement in ["0", "05", "6", "", "+5"] {
        let shape = [
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            "1",
            "--writer-fd",
            replacement,
        ];
        assert!(matches!(
            parse(HelperPlatform::Unix, &shape),
            Err(HelperGrammarRefusal::Grammar)
        ));
    }
}

#[test]
fn windows_mint_grammar_is_closed_and_writer_is_typed_out_of_band() {
    assert_closed_shape(
        HelperPlatform::Windows,
        &[
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            "1",
            "--writer-handle",
            "99",
        ],
    );
    let invocation = parse(
        HelperPlatform::Windows,
        &[
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            "1",
            "--writer-handle",
            "99",
        ],
    );
    let Ok(LocalHelperInvocation::ServiceAccountMint {
        id,
        expires_at,
        writer: MintWriterCapability::Windows(writer),
    }) = invocation
    else {
        panic!("exact Windows mint grammar was refused")
    };
    assert_eq!(id.as_str(), "bot");
    assert_eq!(expires_at.value(), 1);
    assert_eq!(writer.native_value(), 99);

    for shape in [
        vec![
            "local",
            "service-account-mint",
            "--expires-at",
            "1",
            "--id",
            "bot",
            "--writer-handle",
            "99",
        ],
        vec![
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            "1",
            "--writer-fd",
            "5",
        ],
        vec![
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            "1",
            "--writer-handle",
            "99",
            "extra",
        ],
    ] {
        assert!(matches!(
            parse(HelperPlatform::Windows, &shape),
            Err(HelperGrammarRefusal::Grammar)
        ));
    }
}

#[test]
fn service_account_id_and_expiry_bounds_are_exact() {
    let id64 = "a".repeat(64);
    for id in ["a", "A0_-", id64.as_str()] {
        let invocation = [
            OsString::from("local"),
            OsString::from("service-account-mint"),
            OsString::from("--id"),
            OsString::from(id),
            OsString::from("--expires-at"),
            OsString::from("1"),
            OsString::from("--writer-fd"),
            OsString::from("5"),
        ];
        assert!(local_helper::parse_local_helper(HelperPlatform::Unix, &invocation).is_ok());
    }
    let id65 = "a".repeat(65);
    for id in ["", "a.b", "a/b", "é", id65.as_str()] {
        let invocation = [
            OsString::from("local"),
            OsString::from("service-account-mint"),
            OsString::from("--id"),
            OsString::from(id),
            OsString::from("--expires-at"),
            OsString::from("1"),
            OsString::from("--writer-fd"),
            OsString::from("5"),
        ];
        assert!(matches!(
            local_helper::parse_local_helper(HelperPlatform::Unix, &invocation),
            Err(HelperGrammarRefusal::ServiceAccountId)
        ));
    }

    for expiry in ["", "0", "00", "01", "+1", "-1", "9223372036854775808"] {
        let shape = [
            "local",
            "service-account-mint",
            "--id",
            "bot",
            "--expires-at",
            expiry,
            "--writer-fd",
            "5",
        ];
        assert!(matches!(
            parse(HelperPlatform::Unix, &shape),
            Err(HelperGrammarRefusal::ExpiresAt)
        ));
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_scalar_arguments_refuse_without_lossy_conversion() {
    use std::os::unix::ffi::OsStringExt;

    let mint = vec![
        OsString::from("local"),
        OsString::from("service-account-mint"),
        OsString::from("--id"),
        OsString::from_vec(vec![0xff]),
        OsString::from("--expires-at"),
        OsString::from("1"),
        OsString::from("--writer-fd"),
        OsString::from("5"),
    ];
    assert!(matches!(
        local_helper::parse_local_helper(HelperPlatform::Unix, &mint),
        Err(HelperGrammarRefusal::ServiceAccountId)
    ));

    let vendor = vec![
        OsString::from("local"),
        OsString::from("vendor-secret"),
        OsString::from("--request-handle"),
        OsString::from_vec(vec![0xff]),
        OsString::from("--response-handle"),
        OsString::from("2"),
    ];
    assert!(matches!(
        local_helper::parse_local_helper(HelperPlatform::Windows, &vendor),
        Err(HelperGrammarRefusal::Handle)
    ));
}

fn pipe(direction: PipeDirection, identity: u128) -> PipeCapabilityFacts {
    PipeCapabilityFacts {
        anonymous_pipe: true,
        direction,
        pipe_identity: identity,
    }
}

#[test]
fn capability_validation_closes_the_unix_and_windows_sets() {
    let unix = UnixVendorCapabilityFacts {
        request: pipe(PipeDirection::Read, 1),
        response: pipe(PipeDirection::Write, 2),
        fd5_closed: true,
        all_other_nonstandard_fds_closed: true,
    };
    assert_eq!(validate_unix_vendor_capabilities(&unix), Ok(()));

    let windows = WindowsVendorCapabilityFacts {
        request: pipe(PipeDirection::Read, 3),
        response: pipe(PipeDirection::Write, 4),
        inherited_handle_count: 2,
    };
    assert_eq!(validate_windows_vendor_capabilities(&windows), Ok(()));

    let bad_closure = UnixVendorCapabilityFacts {
        request: pipe(PipeDirection::Read, 1),
        response: pipe(PipeDirection::Write, 2),
        fd5_closed: false,
        all_other_nonstandard_fds_closed: true,
    };
    assert_eq!(
        validate_unix_vendor_capabilities(&bad_closure),
        Err(CapabilityRefusal::Closure)
    );

    let extra_handle = WindowsVendorCapabilityFacts {
        request: pipe(PipeDirection::Read, 1),
        response: pipe(PipeDirection::Write, 2),
        inherited_handle_count: 3,
    };
    assert_eq!(
        validate_windows_vendor_capabilities(&extra_handle),
        Err(CapabilityRefusal::Closure)
    );
}

#[test]
fn capability_validation_rejects_type_direction_and_aliasing() {
    for (request_direction, response_direction) in [
        (PipeDirection::Write, PipeDirection::Write),
        (PipeDirection::Read, PipeDirection::Read),
        (PipeDirection::Other, PipeDirection::Write),
        (PipeDirection::Read, PipeDirection::Other),
    ] {
        let facts = UnixVendorCapabilityFacts {
            request: pipe(request_direction, 1),
            response: pipe(response_direction, 2),
            fd5_closed: true,
            all_other_nonstandard_fds_closed: true,
        };
        assert_eq!(
            validate_unix_vendor_capabilities(&facts),
            Err(CapabilityRefusal::Direction)
        );
    }

    let non_pipe = UnixVendorCapabilityFacts {
        request: PipeCapabilityFacts {
            anonymous_pipe: false,
            direction: PipeDirection::Read,
            pipe_identity: 1,
        },
        response: pipe(PipeDirection::Write, 2),
        fd5_closed: true,
        all_other_nonstandard_fds_closed: true,
    };
    assert_eq!(
        validate_unix_vendor_capabilities(&non_pipe),
        Err(CapabilityRefusal::Direction)
    );

    let aliased = UnixVendorCapabilityFacts {
        request: pipe(PipeDirection::Read, 9),
        response: pipe(PipeDirection::Write, 9),
        fd5_closed: true,
        all_other_nonstandard_fds_closed: true,
    };
    assert_eq!(
        validate_unix_vendor_capabilities(&aliased),
        Err(CapabilityRefusal::Duplicate)
    );
}

#[test]
fn frame_and_deadline_bounds_are_exact() {
    assert_eq!(
        validate_complete_frame_size(11),
        Err(FrameSizeRefusal::Truncated)
    );
    assert_eq!(validate_complete_frame_size(12), Ok(()));
    assert_eq!(validate_complete_frame_size(MAX_HELPER_FRAME_BYTES), Ok(()));
    assert_eq!(
        validate_complete_frame_size(MAX_HELPER_FRAME_BYTES + 1),
        Err(FrameSizeRefusal::TooLarge)
    );
    assert_eq!(HELPER_SETUP_DEADLINE, Duration::from_secs(5));
    assert_eq!(HELPER_RESULT_DEADLINE, Duration::from_secs(335));
    assert_eq!(UNIX_VENDOR_REQUEST_FD, 6);
    assert_eq!(UNIX_VENDOR_RESPONSE_FD, 7);
    assert_eq!(UNIX_MINT_WRITER_FD, 5);
}

struct RefusingEndpoint;

impl LocalHelperEndpointPort for RefusingEndpoint {
    type Error = ();

    fn execute(&mut self, _invocation: LocalHelperInvocation) -> Result<HelperExit, Self::Error> {
        Err(())
    }
}

#[test]
fn exit_and_endpoint_ports_have_no_third_status_or_serialized_handle_channel() {
    assert_eq!(HelperExit::TerminalFrameWritten.code(), 0);
    assert_eq!(HelperExit::CapabilityOrTransportFailure.code(), 1);
    let mut endpoint = RefusingEndpoint;
    let invocation = parse(HelperPlatform::Unix, &["local", "vendor-secret"])
        .expect("fixture grammar must be valid");
    assert!(endpoint.execute(invocation).is_err());

    fn type_checks_without_debug_or_serde(
        _id: Option<ServiceAccountId>,
        _expiry: Option<ExpiresAt>,
    ) {
    }
    type_checks_without_debug_or_serde(None, None);
}
