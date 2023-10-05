//! To build the binary:
//!
//!     `cargo build --release --bin skip`
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

use plonky2x::backend::circuit::Circuit;
use plonky2x::backend::function::VerifiableFunction;
use plonky2x::frontend::hint::simple::hint::Hint;
use plonky2x::frontend::uint::uint64::U64Variable;
use plonky2x::frontend::vars::{ValueStream, VariableStream};
use plonky2x::prelude::{
    ArrayVariable, BoolVariable, Bytes32Variable, CircuitBuilder, PlonkParameters,
};
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;

use crate::builder::verify::TendermintVerify;
use crate::input::InputDataFetcher;
use crate::variables::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkipOffchainInputs<const MAX_VALIDATOR_SET_SIZE: usize> {}

impl<const MAX_VALIDATOR_SET_SIZE: usize, L: PlonkParameters<D>, const D: usize> Hint<L, D>
    for SkipOffchainInputs<MAX_VALIDATOR_SET_SIZE>
{
    fn hint(&self, input_stream: &mut ValueStream<L, D>, output_stream: &mut ValueStream<L, D>) {
        let trusted_header_hash = input_stream.read_value::<Bytes32Variable>();
        let trusted_block = input_stream.read_value::<U64Variable>();
        let target_block = input_stream.read_value::<U64Variable>();
        let mut data_fetcher = InputDataFetcher::new();
        let rt = Runtime::new().expect("failed to create tokio runtime");
        let result = rt.block_on(async {
            data_fetcher
                .get_skip_inputs::<MAX_VALIDATOR_SET_SIZE, L::Field>(
                    trusted_block,
                    trusted_header_hash,
                    target_block,
                )
                .await
        });
        output_stream
            .write_value::<ArrayVariable<ValidatorVariable, MAX_VALIDATOR_SET_SIZE>>(result.0); // target_block_validators
        output_stream.write_value::<Bytes32Variable>(result.1.into()); // target_header
        output_stream.write_value::<BoolVariable>(result.2); // round_present
        output_stream.write_value::<HeightProofVariable>(result.3); // block_height_proof
        output_stream.write_value::<HashInclusionProofVariable>(result.4); // validators_hash_proof
        output_stream.write_value::<Bytes32Variable>(result.5.into()); // trusted_header
        output_stream.write_value::<HashInclusionProofVariable>(result.6); // trusted_header_validators_hash_proof
        output_stream
            .write_value::<ArrayVariable<ValidatorHashFieldVariable, MAX_VALIDATOR_SET_SIZE>>(
                result.7,
            ); // trusted_header_validators_hash_fields
    }
}

pub struct SkipCircuit<const MAX_VALIDATOR_SET_SIZE: usize> {
    _config: usize,
}

impl<const MAX_VALIDATOR_SET_SIZE: usize> Circuit for SkipCircuit<MAX_VALIDATOR_SET_SIZE> {
    fn define<L: PlonkParameters<D>, const D: usize>(builder: &mut CircuitBuilder<L, D>) {
        let trusted_header_hash = builder.evm_read::<Bytes32Variable>();
        let trusted_block = builder.evm_read::<U64Variable>();
        let target_block = builder.evm_read::<U64Variable>();

        let mut input_stream = VariableStream::new();
        input_stream.write(&trusted_header_hash);
        input_stream.write(&trusted_block);
        input_stream.write(&target_block);
        let output_stream = builder.hint(
            input_stream,
            SkipOffchainInputs::<MAX_VALIDATOR_SET_SIZE> {},
        );
        let target_block_validators =
            output_stream.read::<ArrayVariable<ValidatorVariable, MAX_VALIDATOR_SET_SIZE>>(builder);
        let target_header = output_stream.read::<Bytes32Variable>(builder);
        let round_present = output_stream.read::<BoolVariable>(builder);
        let target_header_block_height_proof = output_stream.read::<HeightProofVariable>(builder);
        let target_header_validators_hash_proof =
            output_stream.read::<HashInclusionProofVariable>(builder);
        let trusted_header = output_stream.read::<Bytes32Variable>(builder);
        let trusted_header_validators_hash_proof =
            output_stream.read::<HashInclusionProofVariable>(builder);
        let trusted_header_validators_hash_fields = output_stream
            .read::<ArrayVariable<ValidatorHashFieldVariable, MAX_VALIDATOR_SET_SIZE>>(builder);

        builder.skip(
            &target_block_validators,
            &target_header,
            &target_header_block_height_proof,
            &target_header_validators_hash_proof,
            &round_present,
            trusted_header,
            &trusted_header_validators_hash_proof,
            &trusted_header_validators_hash_fields,
        );
        builder.evm_write(target_header);
    }

    fn register_generators<L: PlonkParameters<D>, const D: usize>(
        generator_registry: &mut plonky2x::prelude::HintRegistry<L, D>,
    ) where
        <<L as PlonkParameters<D>>::Config as plonky2::plonk::config::GenericConfig<D>>::Hasher:
            plonky2::plonk::config::AlgebraicHasher<L::Field>,
    {
        generator_registry.register_hint::<SkipOffchainInputs<MAX_VALIDATOR_SET_SIZE>>();
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use ethers::types::H256;
    use ethers::utils::hex;
    use plonky2x::backend::circuit::PublicInput;
    use plonky2x::prelude::{DefaultBuilder, GateRegistry, HintRegistry};

    use super::*;

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_skip_serialization() {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();

        const MAX_VALIDATOR_SET_SIZE: usize = 2;
        let mut builder = DefaultBuilder::new();

        log::debug!("Defining circuit");
        SkipCircuit::<MAX_VALIDATOR_SET_SIZE>::define(&mut builder);
        let circuit = builder.build();
        log::debug!("Done building circuit");

        let mut hint_registry = HintRegistry::new();
        let mut gate_registry = GateRegistry::new();
        SkipCircuit::<MAX_VALIDATOR_SET_SIZE>::register_generators(&mut hint_registry);
        SkipCircuit::<MAX_VALIDATOR_SET_SIZE>::register_gates(&mut gate_registry);

        circuit.test_serializers(&gate_registry, &hint_registry);
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_skip_circuit_with_input_bytes() {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();

        const MAX_VALIDATOR_SET_SIZE: usize = 4;
        // This is from block 3000 with requested block 3100
        let input_bytes = hex::decode(
            "a8512f18c34b70e1533cfd5aa04f251fcb0d7be56ec570051fbad9bdb9435e6a0000000000000bb80000000000000c1c",
        )
        .unwrap();

        let mut builder = DefaultBuilder::new();

        log::debug!("Defining circuit");
        SkipCircuit::<MAX_VALIDATOR_SET_SIZE>::define(&mut builder);

        log::debug!("Building circuit");
        let circuit = builder.build();
        log::debug!("Done building circuit");

        let input = PublicInput::Bytes(input_bytes);
        let (_proof, mut output) = circuit.prove(&input);
        let next_header = output.evm_read::<Bytes32Variable>();
        println!("next_header {:?}", next_header);
    }

    fn test_skip_template<const MAX_VALIDATOR_SET_SIZE: usize>(
        trusted_header: [u8; 32],
        trusted_block: u64,
        target_block: u64,
    ) {
        env::set_var("RUST_LOG", "debug");
        env_logger::try_init().unwrap_or_default();
        env::set_var("RPC_MOCHA_4", "fixture"); // Use fixture during testing

        let mut builder = DefaultBuilder::new();

        log::debug!("Defining circuit");
        SkipCircuit::<MAX_VALIDATOR_SET_SIZE>::define(&mut builder);

        log::debug!("Building circuit");
        let circuit = builder.build();
        log::debug!("Done building circuit");

        let mut input = circuit.input();
        input.evm_write::<Bytes32Variable>(H256::from_slice(trusted_header.as_slice()));
        input.evm_write::<U64Variable>(trusted_block);
        input.evm_write::<U64Variable>(target_block);

        log::debug!("Generating proof");
        let (proof, mut output) = circuit.prove(&input);
        log::debug!("Done generating proof");

        circuit.verify(&proof, &input, &output);
        let target_header = output.evm_read::<Bytes32Variable>();
        println!("target_header {:?}", target_header);
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_skip_small() {
        const MAX_VALIDATOR_SET_SIZE: usize = 4;
        let trusted_header: [u8; 32] =
            hex::decode("A0123D5E4B8B8888A61F931EE2252D83568B97C223E0ECA9795B29B8BD8CBA2D")
                .unwrap()
                .try_into()
                .unwrap();
        let trusted_height = 10000u64;
        let target_height = 10500u64;
        test_skip_template::<MAX_VALIDATOR_SET_SIZE>(trusted_header, trusted_height, target_height)
    }

    #[test]
    #[cfg_attr(feature = "ci", ignore)]
    fn test_skip_large() {
        const MAX_VALIDATOR_SET_SIZE: usize = 128;
        let trusted_header: [u8; 32] =
            hex::decode("935786C7F889013D6B0D8DE8B11286DDB8DDE476A312FC5578FDC53985DC3035")
                .unwrap()
                .try_into()
                .unwrap();
        let trusted_height = 15000u64;
        let target_block = 50000u64;
        test_skip_template::<MAX_VALIDATOR_SET_SIZE>(trusted_header, trusted_height, target_block)
    }
}