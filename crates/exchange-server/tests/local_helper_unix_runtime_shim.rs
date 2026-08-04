//! Namespace-only shim for the direct `local_helper_unix.rs` integration-test include.
//!
//! The test exercises `MintTransfer` through its own `CaptureMint`, never the production-only FD5
//! owner. These uninhabited adapters therefore supply no transfer behavior: entering either method
//! is a test failure. The server binary continues to compile and use the private production types.

pub(crate) mod service_account_handoff {
    pub(crate) mod unix_transfer {
        use std::os::unix::net::UnixStream;

        pub(crate) enum HelperWriter {}

        #[derive(Debug)]
        pub(crate) enum WriterRefusal {}

        pub(crate) type UnixHandoffError = WriterRefusal;

        impl HelperWriter {
            pub(crate) fn inherited_fd5() -> Result<Self, UnixHandoffError> {
                panic!("the integration harness must inject MintTransfer")
            }

            pub(crate) fn transfer_mint(
                self,
                _stream: &UnixStream,
                _mint_frame: &[u8],
            ) -> Result<(), UnixHandoffError> {
                match self {}
            }
        }
    }
}
