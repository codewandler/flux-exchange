//! Namespace-only shim for the direct `local_helper_unix.rs` integration-test include.
//!
//! The test exercises `MintTransfer` through its own `CaptureMint`, never the production FD5
//! owner. These concrete refusal adapters keep the production mint branch type-checkable under
//! clippy while making accidental entry an explicit harness failure.

pub(crate) mod service_account_handoff {
    pub(crate) mod unix_transfer {
        use std::os::unix::net::UnixStream;

        pub(crate) struct HelperWriter;

        #[derive(Debug)]
        pub(crate) enum WriterRefusal {
            Unsupported,
        }

        pub(crate) type UnixHandoffError = WriterRefusal;

        impl HelperWriter {
            pub(crate) fn inherited_fd5() -> Result<Self, UnixHandoffError> {
                Err(WriterRefusal::Unsupported)
            }

            pub(crate) fn transfer_mint(
                self,
                _stream: &UnixStream,
                _mint_frame: &[u8],
            ) -> Result<(), UnixHandoffError> {
                Err(WriterRefusal::Unsupported)
            }
        }
    }
}
