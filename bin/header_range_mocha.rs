//! To build the binary:
//!
//!     `cargo build --release --bin header_range_mocha`
//!
//! To build the circuit:
//!
//!     `./target/release/circuit_function_field build`
//!
//! To prove the circuit using evm io:
//!
//!    `./target/release/circuit_function_evm prove --input-json src/bin/circuit_function_evm_input.json`
//!
//! Note that this circuit will not work with field-based io.
//!
//!
//!
use blobstreamx::config::Mocha4BlobstreamXConfig;
use blobstreamx::consts::{BATCH_SIZE, NB_MAP_JOBS};
use blobstreamx::header_range::CombinedSkipCircuit;
use plonky2x::backend::function::Plonky2xFunction;
use tendermintx::config::MOCHA_4_CHAIN_ID_SIZE_BYTES;

fn main() {
    const VALIDATOR_SET_SIZE_MAX: usize = 100;
    CombinedSkipCircuit::<
        VALIDATOR_SET_SIZE_MAX,
        MOCHA_4_CHAIN_ID_SIZE_BYTES,
        Mocha4BlobstreamXConfig,
        NB_MAP_JOBS,
        BATCH_SIZE,
    >::entrypoint();
}
