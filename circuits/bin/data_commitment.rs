//! To build the binary:
//!
//!     `cargo build --release --bin data_commitment`
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
use std::env;

use celestia::commitment::DataCommitment;
use celestia::input_data::utils::convert_to_h256;
use celestia::input_data::InputDataFetcher;
use celestia::variables::{DataCommitmentProofValueType, DataCommitmentProofVariable};
use ethers::types::H256;
use plonky2x::backend::circuit::Circuit;
use plonky2x::backend::function::VerifiableFunction;
use plonky2x::frontend::hint::simple::hint::Hint;
use plonky2x::frontend::uint::uint64::U64Variable;
use plonky2x::frontend::vars::{ValueStream, VariableStream};
use plonky2x::prelude::{Bytes32Variable, CircuitBuilder, PlonkParameters};
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime; // TODO: re-export this instead of this path
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataCommitmentOffchainInputs<const MAX_LEAVES: usize> {}

impl<const MAX_LEAVES: usize, L: PlonkParameters<D>, const D: usize> Hint<L, D>
    for DataCommitmentOffchainInputs<MAX_LEAVES>
{
    fn hint(&self, input_stream: &mut ValueStream<L, D>, output_stream: &mut ValueStream<L, D>) {
        let start_block = input_stream.read_value::<U64Variable>();
        let start_header_hash = input_stream.read_value::<Bytes32Variable>();
        let end_block = input_stream.read_value::<U64Variable>();
        let end_header_hash = input_stream.read_value::<Bytes32Variable>();

        let mut data_fetcher = InputDataFetcher::new();

        let rt = Runtime::new().expect("failed to create tokio runtime");
        let result = rt.block_on(async {
            data_fetcher
                .get_data_commitment_inputs::<MAX_LEAVES, L::Field>(
                    start_block.as_u64(),
                    start_header_hash,
                    end_block.as_u64(),
                    end_header_hash,
                )
                .await
        });
        let data_comm_proof = DataCommitmentProofValueType {
            data_hashes: convert_to_h256(result.0),
            start_block_height: start_block,
            start_header: start_header_hash,
            end_block_height: end_block,
            end_header: end_header_hash,
            data_hash_proofs: result.1,
            prev_header_proofs: result.2,
        };
        // Write the inputs to the data commitment circuit.
        output_stream.write_value::<DataCommitmentProofVariable<MAX_LEAVES>>(data_comm_proof);
        // Write the expected data commitment.
        output_stream.write_value::<Bytes32Variable>(H256(result.3));
    }
}

struct DataCommitmentCircuit<const MAX_LEAVES: usize> {
    _config: usize,
}

impl<const MAX_LEAVES: usize> Circuit for DataCommitmentCircuit<MAX_LEAVES> {
    fn define<L: PlonkParameters<D>, const D: usize>(builder: &mut CircuitBuilder<L, D>) {
        let start_block_number = builder.evm_read::<U64Variable>();
        let start_header_hash = builder.evm_read::<Bytes32Variable>();
        let end_block_number = builder.evm_read::<U64Variable>();
        let end_header_hash = builder.evm_read::<Bytes32Variable>();

        let mut input_stream = VariableStream::new();
        input_stream.write(&start_block_number);
        input_stream.write(&start_header_hash);
        input_stream.write(&end_block_number);
        input_stream.write(&end_header_hash);
        let output_stream =
            builder.hint(input_stream, DataCommitmentOffchainInputs::<MAX_LEAVES> {});
        let data_comm_proof =
            output_stream.read::<DataCommitmentProofVariable<MAX_LEAVES>>(builder);

        let expected_data_commitment = output_stream.read::<Bytes32Variable>(builder);

        let data_commitment = builder.prove_data_commitment::<MAX_LEAVES>(data_comm_proof);

        builder.assert_is_equal(data_commitment, expected_data_commitment);

        builder.evm_write(data_commitment);
    }

    fn register_generators<L: PlonkParameters<D>, const D: usize>(
        generator_registry: &mut plonky2x::prelude::HintRegistry<L, D>,
    ) where
        <<L as PlonkParameters<D>>::Config as plonky2::plonk::config::GenericConfig<D>>::Hasher:
            plonky2::plonk::config::AlgebraicHasher<L::Field>,
    {
        generator_registry.register_hint::<DataCommitmentOffchainInputs<MAX_LEAVES>>();
    }
}

fn main() {
    // Celestia's maxmimum data commitment size is 1000: https://github.com/celestiaorg/celestia-core/blob/6933af1ead0ddf4a8c7516690e3674c6cdfa7bd8/pkg/consts/consts.go#L44.
    let env_max_leaves = env::var("MAX_LEAVES").unwrap_or(0.to_string());

    if env_max_leaves == 1024.to_string() {
        const MAX_LEAVES: usize = 1024;
        VerifiableFunction::<DataCommitmentCircuit<MAX_LEAVES>>::entrypoint();
    } else if env_max_leaves == 256.to_string() {
        const MAX_LEAVES: usize = 256;
        VerifiableFunction::<DataCommitmentCircuit<MAX_LEAVES>>::entrypoint();
    } else if env_max_leaves == 4.to_string() {
        const MAX_LEAVES: usize = 4;
        VerifiableFunction::<DataCommitmentCircuit<MAX_LEAVES>>::entrypoint();
    } else {
        panic!("MAX_LEAVES must be set to 1024, 256, or 4");
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use plonky2x::prelude::{DefaultBuilder, GateRegistry, HintRegistry};
    use subtle_encoding::hex;

    use super::*;

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_data_commitment_serialization() {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();

        const MAX_LEAVES: usize = 2;
        let mut builder = DefaultBuilder::new();

        log::debug!("Defining circuit");
        DataCommitmentCircuit::<MAX_LEAVES>::define(&mut builder);
        let circuit = builder.build();
        log::debug!("Done building circuit");

        let mut hint_registry = HintRegistry::new();
        let mut gate_registry = GateRegistry::new();
        DataCommitmentCircuit::<MAX_LEAVES>::register_generators(&mut hint_registry);
        DataCommitmentCircuit::<MAX_LEAVES>::register_gates(&mut gate_registry);

        circuit.test_serializers(&gate_registry, &hint_registry);
    }

    fn test_data_commitment_template<const MAX_LEAVES: usize>(
        start_block: usize,
        start_header_hash: [u8; 32],
        end_block: usize,
        end_header_hash: [u8; 32],
    ) {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();
        // env::set_var("RPC_MOCHA_4", "fixture"); // Use fixture during testing

        let mut builder = DefaultBuilder::new();

        log::debug!("Defining circuit");
        DataCommitmentCircuit::<MAX_LEAVES>::define(&mut builder);

        log::debug!("Building circuit");
        let circuit = builder.build();
        log::debug!("Done building circuit");

        let mut input = circuit.input();

        input.evm_write::<U64Variable>(start_block.into());
        input.evm_write::<Bytes32Variable>(H256::from_slice(start_header_hash.as_slice()));
        input.evm_write::<U64Variable>(end_block.into());
        input.evm_write::<Bytes32Variable>(H256::from_slice(end_header_hash.as_slice()));

        log::debug!("Generating proof");
        let (proof, mut output) = circuit.prove(&input);
        log::debug!("Done generating proof");

        circuit.verify(&proof, &input, &output);
        let data_commitment = output.evm_read::<Bytes32Variable>();
        println!("data_commitment {:?}", data_commitment);
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_data_commitment_small() {
        // Test variable length NUM_BLOCKS.
        const MAX_LEAVES: usize = 8;
        const NUM_BLOCKS: usize = 4;

        let start_block = 10000u64;
        let start_header_hash =
            hex::decode_upper("A0123D5E4B8B8888A61F931EE2252D83568B97C223E0ECA9795B29B8BD8CBA2D")
                .unwrap();
        let end_block = start_block + NUM_BLOCKS as u64;
        let end_header_hash =
            hex::decode_upper("FCDA37FA6306C77737DD911E6101B612E2DBD837F29ED4F4E1C30919FBAC9D05")
                .unwrap();

        test_data_commitment_template::<MAX_LEAVES>(
            start_block as usize,
            start_header_hash.as_slice().try_into().unwrap(),
            end_block as usize,
            end_header_hash.as_slice().try_into().unwrap(),
        );
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_data_commitment_large() {
        // Test variable length NUM_BLOCKS.
        const MAX_LEAVES: usize = 1024;
        const NUM_BLOCKS: usize = 4;

        let start_block = 10000u64;
        let start_header_hash =
            hex::decode_upper("A0123D5E4B8B8888A61F931EE2252D83568B97C223E0ECA9795B29B8BD8CBA2D")
                .unwrap();
        let end_block = start_block + NUM_BLOCKS as u64;
        let end_header_hash =
            hex::decode_upper("FCDA37FA6306C77737DD911E6101B612E2DBD837F29ED4F4E1C30919FBAC9D05")
                .unwrap();

        test_data_commitment_template::<MAX_LEAVES>(
            start_block as usize,
            start_header_hash.as_slice().try_into().unwrap(),
            end_block as usize,
            end_header_hash.as_slice().try_into().unwrap(),
        );
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_data_commitment_smart_contract() {
        // Test variable length NUM_BLOCKS.
        const MAX_LEAVES: usize = 256;
        const NUM_BLOCKS: usize = 100;

        let start_block = 100000u64;
        let start_header_hash =
            hex::decode_upper("0C1D96912ACE4102C620EC6223E4A457D01ABC9CEC70B7149A10410472D6D60E")
                .unwrap();
        let end_block = start_block + NUM_BLOCKS as u64;
        let end_header_hash =
            hex::decode_upper("400773BF4613E2F0311DD382DB3B2278B6442560A7AD6627984799D2FC4F0DF9")
                .unwrap();

        test_data_commitment_template::<MAX_LEAVES>(
            start_block as usize,
            start_header_hash.as_slice().try_into().unwrap(),
            end_block as usize,
            end_header_hash.as_slice().try_into().unwrap(),
        );
    }
}
