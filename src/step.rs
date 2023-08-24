//! The protobuf encoding of a Tendermint validator is a deterministic function of the validator's
//! public key (32 bytes) and voting power (int64). The encoding is as follows in bytes:
//
//!     10 34 10 32 <pubkey> 16 <varint>
//
//! The `pubkey` is encoded as the raw list of bytes used in the public key. The `varint` is
//! encoded using protobuf's default integer encoding, which consist of 7 bit payloads. You can
//! read more about them here: https://protobuf.dev/programming-guides/encoding/#varints.
use curta::math::extension::CubicParameters;
use plonky2::field::extension::Extendable;
use plonky2::iop::target::BoolTarget;
use plonky2::iop::target::Target;
use plonky2::plonk::config::AlgebraicHasher;
use plonky2::plonk::config::GenericConfig;
use plonky2::{hash::hash_types::RichField, plonk::circuit_builder::CircuitBuilder};
use plonky2x::ecc::ed25519::curve::curve_types::Curve;
use plonky2x::ecc::ed25519::curve::ed25519::Ed25519;
use plonky2x::ecc::ed25519::gadgets::curve::CircuitBuilderCurve;
use plonky2x::ecc::ed25519::gadgets::eddsa::{EDDSAPublicKeyTarget, EDDSASignatureTarget};
use plonky2x::num::nonnative::nonnative::CircuitBuilderNonNative;
use plonky2x::num::u32::gadgets::arithmetic_u32::{CircuitBuilderU32, U32Target};

use crate::signature::TendermintSignature;
use crate::utils::EncBlockIDTarget;
use crate::utils::PROTOBUF_BLOCK_ID_SIZE_BITS;
use crate::utils::{
    EncTendermintHashTarget, I64Target, MarshalledValidatorTarget, TendermintHashTarget,
    ValidatorMessageTarget, HASH_SIZE_BITS, HEADER_PROOF_DEPTH, PROTOBUF_HASH_SIZE_BITS,
    VALIDATOR_MESSAGE_BYTES_LENGTH_MAX,
};
use crate::validator::TendermintMarshaller;
use crate::voting::TendermintVoting;

#[derive(Debug, Clone)]
pub struct ValidatorTarget<C: Curve> {
    pubkey: EDDSAPublicKeyTarget<C>,
    signature: EDDSASignatureTarget<C>,
    message: ValidatorMessageTarget,
    message_bit_length: Target,
    voting_power: I64Target,
    validator_byte_length: Target,
    enabled: BoolTarget,
    signed: BoolTarget,
}

/// The protobuf-encoded leaf (a hash), and it's corresponding proof and path indices against the header.
#[derive(Debug, Clone)]
pub struct HashInclusionProofTarget {
    enc_leaf: EncTendermintHashTarget,
    // Path and proof should have a fixed length of HEADER_PROOF_DEPTH.
    path: Vec<BoolTarget>,
    proof: Vec<TendermintHashTarget>,
}

/// The protobuf-encoded leaf (a tendermint block ID), and it's corresponding proof and path indices against the header.
#[derive(Debug, Clone)]
pub struct BlockIDInclusionProofTarget {
    enc_leaf: EncBlockIDTarget,
    // Path and proof should have a fixed length of HEADER_PROOF_DEPTH.
    path: Vec<BoolTarget>,
    proof: Vec<TendermintHashTarget>,
}

#[derive(Debug, Clone)]
pub struct CelestiaBlockProofTarget<C: Curve> {
    validators: Vec<ValidatorTarget<C>>,
    header: TendermintHashTarget,
    prev_header: TendermintHashTarget,
    data_hash_proof: HashInclusionProofTarget,
    validator_hash_proof: HashInclusionProofTarget,
    next_validators_hash_proof: HashInclusionProofTarget,
    last_block_id_proof: BlockIDInclusionProofTarget,
    round_present: BoolTarget,
}

pub trait TendermintStep<F: RichField + Extendable<D>, const D: usize> {
    type Curve: Curve;

    /// Verifies a Tendermint consensus block.
    fn step<E: CubicParameters<F>, C: GenericConfig<D, F = F, FE = F::Extension> + 'static, const VALIDATOR_SET_SIZE_MAX: usize>(
        &mut self,
        validators: &Vec<ValidatorTarget<Self::Curve>>,
        header: &TendermintHashTarget,
        prev_header: &TendermintHashTarget,
        data_hash_proof: &HashInclusionProofTarget,
        validator_hash_proof: &HashInclusionProofTarget,
        next_validators_hash_proof: &HashInclusionProofTarget,
        last_block_id_proof: &BlockIDInclusionProofTarget,
        round_present: &BoolTarget,
    ) where
        <C as GenericConfig<D>>::Hasher: AlgebraicHasher<F>;
}

impl<F: RichField + Extendable<D>, const D: usize> TendermintStep<F, D> for CircuitBuilder<F, D> {
    type Curve = Ed25519;

    fn step<E: CubicParameters<F>, C: GenericConfig<D, F = F, FE = F::Extension> + 'static, const VALIDATOR_SET_SIZE_MAX: usize>(
        &mut self,
        validators: &Vec<ValidatorTarget<Self::Curve>>,
        header: &TendermintHashTarget,
        prev_header: &TendermintHashTarget,
        data_hash_proof: &HashInclusionProofTarget,
        validator_hash_proof: &HashInclusionProofTarget,
        next_validators_hash_proof: &HashInclusionProofTarget,
        last_block_id_proof: &BlockIDInclusionProofTarget,
        round_present: &BoolTarget,
    ) where
        <C as GenericConfig<D>>::Hasher: AlgebraicHasher<F>,
    {
        let one = self.one();
        let false_t = self._false();
        let true_t = self._true();
        // Verify each of the validators marshal correctly
        // Assumes the validators are sorted in the correct order
        let byte_lengths: Vec<Target> =
            validators.iter().map(|v| v.validator_byte_length).collect();
        let marshalled_validators: Vec<MarshalledValidatorTarget> = validators
            .iter()
            .map(|v| self.marshal_tendermint_validator(&v.pubkey.0, &v.voting_power))
            .collect();
        let validators_signed: Vec<BoolTarget> = validators.iter().map(|v| v.signed).collect();
        let validators_enabled: Vec<BoolTarget> = validators.iter().map(|v| v.enabled).collect();
        let validators_enabled_u32: Vec<U32Target> = validators_enabled
            .iter()
            .map(|v| {
                let zero = self.zero_u32();
                let one = self.one_u32();
                U32Target(self.select(*v, one.0, zero.0))
            })
            .collect();

        let validator_voting_power: Vec<I64Target> =
            validators.iter().map(|v| v.voting_power).collect();

        let mut messages: Vec<Vec<BoolTarget>> =
            validators.iter().map(|v| v.message.0.to_vec()).collect();
        for i in 0..messages.len() {
            messages[i].resize(VALIDATOR_MESSAGE_BYTES_LENGTH_MAX * 8, self._false());
        }

        let messages: Vec<ValidatorMessageTarget> = messages
            .iter()
            .map(|v| ValidatorMessageTarget(v.clone().try_into().unwrap()))
            .collect();

        let message_bit_lengths: Vec<Target> =
            validators.iter().map(|v| v.message_bit_length).collect();

        let signatures: Vec<&EDDSASignatureTarget<Ed25519>> =
            validators.iter().map(|v| &v.signature).collect();
        let pubkeys: Vec<&EDDSAPublicKeyTarget<Ed25519>> =
            validators.iter().map(|v| &v.pubkey).collect();

        // Compute the validators hash
        let validators_hash_target =
            self.hash_validator_set::<VALIDATOR_SET_SIZE_MAX>(&marshalled_validators, &byte_lengths, &validators_enabled);

        /// Start of the hash in protobuf encoded validator hash & last block id
        const HASH_START_BYTE: usize = 2;
        // Assert that computed validator hash matches expected validator hash
        let extracted_hash = self.extract_hash_from_protobuf::<HASH_START_BYTE, PROTOBUF_HASH_SIZE_BITS>(&validator_hash_proof.enc_leaf.0);
        for i in 0..HASH_SIZE_BITS {
            self.connect(
                validators_hash_target.0[i].target,
                extracted_hash.0[i].target,
            );
        }

        let total_voting_power = self.get_total_voting_power::<VALIDATOR_SET_SIZE_MAX>(&validator_voting_power);
        let threshold_numerator = self.constant_u32(2);
        let threshold_denominator = self.constant_u32(3);

        // Assert the accumulated voting power is greater than the threshold
        let check_voting_power_bool = self.check_voting_power::<VALIDATOR_SET_SIZE_MAX>(
            &validator_voting_power,
            &validators_enabled_u32,
            &total_voting_power,
            &threshold_numerator,
            &threshold_denominator,
        );
        self.connect(check_voting_power_bool.target, one);

        // // TODO: Handle dummies
        self.verify_signatures::<E, C>(
            &validators_signed,
            messages,
            message_bit_lengths,
            signatures,
            pubkeys,
        );

        // TODO: Verify that this will work with dummy signatures
        for i in 0..VALIDATOR_SET_SIZE_MAX {
            // Verify that the header is in the message in the correct location
            let hash_in_message =
                self.verify_hash_in_message(&validators[i].message, header, round_present);

            // If the validator is enabled, then the hash should be in the message
            self.connect(hash_in_message.target, validators_signed[i].target);
        }

        // Note: Hardcode the path for each of the leaf proofs (otherwise you can prove arbitrary data in the header)
        let data_hash_path = vec![false_t, true_t, true_t, false_t];
        let val_hash_path = vec![true_t, true_t, true_t, false_t];
        let next_val_hash_path = vec![false_t, false_t, false_t, true_t];
        let last_block_id_path = vec![false_t, false_t, true_t, false_t];

        let data_hash_leaf_hash = self.leaf_hash::<PROTOBUF_HASH_SIZE_BITS>(&data_hash_proof.enc_leaf.0);
        let header_from_data_root_proof = self.get_root_from_merkle_proof::<HEADER_PROOF_DEPTH>(
            &data_hash_proof.proof,
            &data_hash_path,
            &data_hash_leaf_hash,
        );

        let validator_hash_leaf_hash = self.leaf_hash::<PROTOBUF_HASH_SIZE_BITS>(&validator_hash_proof.enc_leaf.0);
        let header_from_validator_root_proof = self.get_root_from_merkle_proof::<HEADER_PROOF_DEPTH>(
            &validator_hash_proof.proof,
            &val_hash_path,
            &validator_hash_leaf_hash,
        );

        let next_validators_hash_leaf_hash = self.leaf_hash::<PROTOBUF_HASH_SIZE_BITS>(&next_validators_hash_proof.enc_leaf.0);
        let header_from_next_validators_root_proof = self.get_root_from_merkle_proof::<HEADER_PROOF_DEPTH>(
            &next_validators_hash_proof.proof,
            &next_val_hash_path,
            &next_validators_hash_leaf_hash,
        );

        let last_block_id_leaf_hash = self.leaf_hash::<PROTOBUF_BLOCK_ID_SIZE_BITS>(&last_block_id_proof.enc_leaf.0);
        let header_from_last_block_id_proof = self.get_root_from_merkle_proof::<HEADER_PROOF_DEPTH>(
            &last_block_id_proof.proof,
            &last_block_id_path,
            &last_block_id_leaf_hash,
        );

        // Confirm that the header from the proof of {validator_hash, next_validators_hash, data_hash, last_block_id} all match the header
        for i in 0..HASH_SIZE_BITS {
            self.connect(header.0[i].target, header_from_data_root_proof.0[i].target);
            self.connect(
                header.0[i].target,
                header_from_validator_root_proof.0[i].target,
            );
            self.connect(
                header.0[i].target,
                header_from_next_validators_root_proof.0[i].target,
            );
            self.connect(
                header.0[i].target,
                header_from_last_block_id_proof.0[i].target,
            );
        }

        // Extract prev header hash from the encoded leaf (starts at second byte)
        let extracted_prev_header_hash = self.extract_hash_from_protobuf::<HASH_START_BYTE, PROTOBUF_BLOCK_ID_SIZE_BITS>(&last_block_id_proof.enc_leaf.0);
        for i in 0..HASH_SIZE_BITS {
            self.connect(
                prev_header.0[i].target,
                extracted_prev_header_hash.0[i].target,
            );
        }
    }
}

fn create_virtual_bool_target_array<F: RichField + Extendable<D>, const D: usize>(
    builder: &mut CircuitBuilder<F, D>,
    size: usize,
) -> Vec<BoolTarget> {
    let mut result = Vec::new();
    for _i in 0..size {
        result.push(builder.add_virtual_bool_target_safe());
    }
    result
}

fn create_virtual_hash_inclusion_proof_target<F: RichField + Extendable<D>, const D: usize, const PROOF_DEPTH: usize>(
    builder: &mut CircuitBuilder<F, D>,
) -> HashInclusionProofTarget {
    let mut proof = Vec::new();
    for _i in 0..PROOF_DEPTH {
        proof.push(TendermintHashTarget(
            create_virtual_bool_target_array(builder, HASH_SIZE_BITS)
                .try_into()
                .unwrap(),
        ));
    }
    HashInclusionProofTarget {
        enc_leaf: EncTendermintHashTarget(
            create_virtual_bool_target_array(builder, PROTOBUF_HASH_SIZE_BITS)
                .try_into()
                .unwrap(),
        ),
        path: create_virtual_bool_target_array(builder, PROOF_DEPTH),
        proof,
    }
}

fn create_virtual_block_id_inclusion_proof_target<F: RichField + Extendable<D>, const D: usize, const PROOF_DEPTH: usize>(
    builder: &mut CircuitBuilder<F, D>,
) -> BlockIDInclusionProofTarget {
    let mut proof = Vec::new();
    for _i in 0..PROOF_DEPTH {
        proof.push(TendermintHashTarget(
            create_virtual_bool_target_array(builder, HASH_SIZE_BITS)
                .try_into()
                .unwrap(),
        ));
    }
    BlockIDInclusionProofTarget {
        enc_leaf: EncBlockIDTarget(
            create_virtual_bool_target_array(builder, PROTOBUF_BLOCK_ID_SIZE_BITS)
                .try_into()
                .unwrap(),
        ),
        path: create_virtual_bool_target_array(builder, PROOF_DEPTH),
        proof,
    }
}

pub fn make_step_circuit<
    F: RichField + Extendable<D>,
    const D: usize,
    C: Curve,
    Config: GenericConfig<D, F = F, FE = F::Extension> + 'static,
    E: CubicParameters<F>,
    const VALIDATOR_SET_SIZE_MAX: usize
>(
    builder: &mut CircuitBuilder<F, D>,
) -> CelestiaBlockProofTarget<Ed25519>
where
    Config::Hasher: AlgebraicHasher<F>,
{
    type Curve = Ed25519;
    let mut validators = Vec::new();
    for _i in 0..VALIDATOR_SET_SIZE_MAX {
        let pubkey = EDDSAPublicKeyTarget(builder.add_virtual_affine_point_target());
        let signature = EDDSASignatureTarget {
            r: builder.add_virtual_affine_point_target(),
            s: builder.add_virtual_nonnative_target(),
        };
        let message =
            create_virtual_bool_target_array(builder, VALIDATOR_MESSAGE_BYTES_LENGTH_MAX * 8);
        let message = ValidatorMessageTarget(message.try_into().unwrap());

        let message_bit_length = builder.add_virtual_target();

        let voting_power = I64Target([
            builder.add_virtual_u32_target(),
            builder.add_virtual_u32_target(),
        ]);
        let validator_byte_length = builder.add_virtual_target();
        let enabled = builder.add_virtual_bool_target_safe();
        let signed = builder.add_virtual_bool_target_safe();

        validators.push(ValidatorTarget::<Curve> {
            pubkey,
            signature,
            message,
            message_bit_length,
            voting_power,
            validator_byte_length,
            enabled,
            signed,
        })
    }

    let header = create_virtual_bool_target_array(builder, HASH_SIZE_BITS);
    let header = TendermintHashTarget(header.try_into().unwrap());

    let prev_header = create_virtual_bool_target_array(builder, HASH_SIZE_BITS);
    let prev_header = TendermintHashTarget(prev_header.try_into().unwrap());

    let data_hash_proof = create_virtual_hash_inclusion_proof_target::<F, D, HEADER_PROOF_DEPTH>(builder);
    let validator_hash_proof = create_virtual_hash_inclusion_proof_target::<F, D, HEADER_PROOF_DEPTH>(builder);
    let next_validators_hash_proof = create_virtual_hash_inclusion_proof_target::<F, D, HEADER_PROOF_DEPTH>(builder);
    let last_block_id_proof = create_virtual_block_id_inclusion_proof_target::<F, D, HEADER_PROOF_DEPTH>(builder);

    let round_present = builder.add_virtual_bool_target_safe();

    builder.step::<E, Config, VALIDATOR_SET_SIZE_MAX>(
        &validators,
        &header,
        &prev_header,
        &data_hash_proof,
        &validator_hash_proof,
        &next_validators_hash_proof,
        &last_block_id_proof,
        &round_present,
    );

    CelestiaBlockProofTarget::<Curve> {
        validators,
        header,
        prev_header,
        data_hash_proof,
        validator_hash_proof,
        next_validators_hash_proof,
        last_block_id_proof,
        round_present,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use curta::math::goldilocks::cubic::GoldilocksCubicParameters;
    use num::BigUint;
    use plonky2::field::goldilocks_field::GoldilocksField;
    use plonky2::field::types::Field;
    use plonky2::{
        iop::witness::{PartialWitness, WitnessWrite},
        plonk::{
            circuit_builder::CircuitBuilder, circuit_data::CircuitConfig,
            config::PoseidonGoldilocksConfig,
        },
    };
    use plonky2x::ecc::ed25519::gadgets::curve::WitnessAffinePoint;
    use plonky2x::num::biguint::WitnessBigUint;
    use plonky2x::num::u32::witness::WitnessU32;

    use plonky2x::ecc::ed25519::curve::curve_types::AffinePoint;
    use plonky2x::ecc::ed25519::field::ed25519_scalar::Ed25519Scalar;

    use crate::inputs::{generate_step_inputs, CelestiaStepBlockProof};
    use crate::utils::{to_be_bits};

    use log;
    use plonky2::timed;
    use plonky2::util::timing::TimingTree;
    use subtle_encoding::hex;

    type C = PoseidonGoldilocksConfig;
    type F = <C as GenericConfig<D>>::F;
    const D: usize = 2;

    #[test]
    fn test_verify_hash_in_message() {
        // This is a test case generated from block 144094 of Celestia's Mocha testnet
        // Block Hash: 8909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c (needs to be lower case)
        // Signed Message (from the last validator): 6b080211de3202000000000022480a208909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c12240801122061263df4855e55fcab7aab0a53ee32cf4f29a1101b56de4a9d249d44e4cf96282a0b089dce84a60610ebb7a81932076d6f6368612d33
        // No round exists in present the message that was signed above

        let header_hash = "8909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c";
        let header_bits = to_be_bits(hex::decode(header_hash).unwrap());

        let signed_message = "6b080211de3202000000000022480a208909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c12240801122061263df4855e55fcab7aab0a53ee32cf4f29a1101b56de4a9d249d44e4cf96282a0b089dce84a60610ebb7a81932076d6f6368612d33";
        let signed_message_bits = to_be_bits(hex::decode(signed_message).unwrap());

        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        let zero = builder._false();

        let mut signed_message_target = [builder._false(); VALIDATOR_MESSAGE_BYTES_LENGTH_MAX * 8];
        for i in 0..signed_message_bits.len() {
            signed_message_target[i] = builder.constant_bool(signed_message_bits[i]);
        }

        let mut header_hash_target = [builder._false(); HASH_SIZE_BITS];
        for i in 0..header_bits.len() {
            header_hash_target[i] = builder.constant_bool(header_bits[i]);
        }

        let result = builder.verify_hash_in_message(
            &ValidatorMessageTarget(signed_message_target),
            &TendermintHashTarget(header_hash_target),
            &zero,
        );

        pw.set_target(result.target, F::ONE);

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        println!("Created proof");

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    fn test_step_template<const VALIDATOR_SET_SIZE_MAX: usize>(block: usize) {
        let _ = env_logger::builder().is_test(true).try_init();
        let mut timing = TimingTree::new("Celestia Header Verify", log::Level::Debug);

        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_ecc_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        type F = GoldilocksField;
        type Curve = Ed25519;
        type E = GoldilocksCubicParameters;
        type C = PoseidonGoldilocksConfig;
        const D: usize = 2;

        println!("Making step circuit");

        let celestia_proof_target =
            make_step_circuit::<GoldilocksField, D, Curve, C, E, VALIDATOR_SET_SIZE_MAX>(&mut builder);

        // Note: Length of output is the closest power of 2 gte the number of validators for this block.
        let celestia_block_proof: CelestiaStepBlockProof = generate_step_inputs(block);
        println!("Generated inputs");
        println!("Number of validators: {}", celestia_block_proof.validators.len());
        timed!(timing, "assigning inputs", {
            // Set target for header
            let header_bits = to_be_bits(celestia_block_proof.header);
            for i in 0..HASH_SIZE_BITS {
                pw.set_bool_target(celestia_proof_target.header.0[i], header_bits[i]);
            }

            // Set target for round present
            pw.set_bool_target(
                celestia_proof_target.round_present,
                celestia_block_proof.round_present,
            );

            // Set the encoded leaf for each of the proofs
            let data_hash_enc_leaf = to_be_bits(celestia_block_proof.data_hash_proof.enc_leaf);
            let val_hash_enc_leaf = to_be_bits(celestia_block_proof.validator_hash_proof.enc_leaf);
            let next_val_hash_enc_leaf =
                to_be_bits(celestia_block_proof.next_validators_hash_proof.enc_leaf);
            let last_block_id_enc_leaf =
                to_be_bits(celestia_block_proof.last_block_id_proof.enc_leaf);

            for i in 0..PROTOBUF_HASH_SIZE_BITS {
                pw.set_bool_target(
                    celestia_proof_target.data_hash_proof.enc_leaf.0[i],
                    data_hash_enc_leaf[i],
                );
                pw.set_bool_target(
                    celestia_proof_target.validator_hash_proof.enc_leaf.0[i],
                    val_hash_enc_leaf[i],
                );
                pw.set_bool_target(
                    celestia_proof_target.next_validators_hash_proof.enc_leaf.0[i],
                    next_val_hash_enc_leaf[i],
                );
            }

            for i in 0..PROTOBUF_BLOCK_ID_SIZE_BITS {
                pw.set_bool_target(
                    celestia_proof_target.last_block_id_proof.enc_leaf.0[i],
                    last_block_id_enc_leaf[i],
                );
            }

            for i in 0..HEADER_PROOF_DEPTH {
                // Set path indices for each of the proof indices
                pw.set_bool_target(
                    celestia_proof_target.data_hash_proof.path[i],
                    celestia_block_proof.data_hash_proof.path[i],
                );
                pw.set_bool_target(
                    celestia_proof_target.validator_hash_proof.path[i],
                    celestia_block_proof.validator_hash_proof.path[i],
                );
                pw.set_bool_target(
                    celestia_proof_target.next_validators_hash_proof.path[i],
                    celestia_block_proof.next_validators_hash_proof.path[i],
                );
                pw.set_bool_target(
                    celestia_proof_target.last_block_id_proof.path[i],
                    celestia_block_proof.last_block_id_proof.path[i],
                );

                let data_hash_aunt =
                    to_be_bits(celestia_block_proof.data_hash_proof.proof[i].to_vec());

                let val_hash_aunt =
                    to_be_bits(celestia_block_proof.validator_hash_proof.proof[i].to_vec());

                let next_val_aunt =
                    to_be_bits(celestia_block_proof.next_validators_hash_proof.proof[i].to_vec());
                let last_block_id_aunt =
                    to_be_bits(celestia_block_proof.last_block_id_proof.proof[i].to_vec());

                // Set aunts for each of the proofs
                for j in 0..HASH_SIZE_BITS {
                    pw.set_bool_target(
                        celestia_proof_target.data_hash_proof.proof[i].0[j],
                        data_hash_aunt[j],
                    );
                    pw.set_bool_target(
                        celestia_proof_target.validator_hash_proof.proof[i].0[j],
                        val_hash_aunt[j],
                    );
                    pw.set_bool_target(
                        celestia_proof_target.next_validators_hash_proof.proof[i].0[j],
                        next_val_aunt[j],
                    );
                    pw.set_bool_target(
                        celestia_proof_target.last_block_id_proof.proof[i].0[j],
                        last_block_id_aunt[j],
                    );
                }
            }

            // Set the targets for each of the validators
            for i in 0..VALIDATOR_SET_SIZE_MAX {
                let validator = &celestia_block_proof.validators[i];
                let signature_bytes = validator.signature.clone().into_bytes();

                let voting_power_lower = (validator.voting_power & ((1 << 32) - 1)) as u32;
                let voting_power_upper = (validator.voting_power >> 32) as u32;

                let pub_key_uncompressed: AffinePoint<Curve> =
                    AffinePoint::new_from_compressed_point(validator.pubkey.as_bytes());

                let sig_r: AffinePoint<Curve> =
                    AffinePoint::new_from_compressed_point(&signature_bytes[0..32]);
                assert!(sig_r.is_valid());

                let sig_s_biguint = BigUint::from_bytes_le(&signature_bytes[32..64]);
                let _sig_s = Ed25519Scalar::from_noncanonical_biguint(sig_s_biguint.clone());

                // Set the targets for the public key
                pw.set_affine_point_target(
                    &celestia_proof_target.validators[i].pubkey.0,
                    &pub_key_uncompressed,
                );

                // Set signature targets
                pw.set_affine_point_target(
                    &celestia_proof_target.validators[i].signature.r,
                    &sig_r,
                );
                pw.set_biguint_target(
                    &celestia_proof_target.validators[i].signature.s.value,
                    &sig_s_biguint,
                );

                let message_bits = to_be_bits(validator.message.clone());
                // Set messages for each of the proofs
                for j in 0..message_bits.len() {
                    pw.set_bool_target(
                        celestia_proof_target.validators[i].message.0[j],
                        message_bits[j],
                    );
                }
                for j in message_bits.len()..VALIDATOR_MESSAGE_BYTES_LENGTH_MAX * 8 {
                    pw.set_bool_target(celestia_proof_target.validators[i].message.0[j], false);
                }

                // Set voting power targets
                pw.set_u32_target(
                    celestia_proof_target.validators[i].voting_power.0[0],
                    voting_power_lower,
                );
                pw.set_u32_target(
                    celestia_proof_target.validators[i].voting_power.0[1],
                    voting_power_upper,
                );

                // Set length targets
                pw.set_target(
                    celestia_proof_target.validators[i].validator_byte_length,
                    F::from_canonical_usize(validator.validator_byte_length),
                );
                let message_bit_length = validator.message_bit_length;

                pw.set_target(
                    celestia_proof_target.validators[i].message_bit_length,
                    F::from_canonical_usize(message_bit_length),
                );

                // Set enabled and signed
                pw.set_bool_target(
                    celestia_proof_target.validators[i].enabled,
                    validator.enabled,
                );
                println!("validator {} signed: {}", i, validator.signed);
                pw.set_bool_target(celestia_proof_target.validators[i].signed, validator.signed);
            }
        });
        let inner_data = builder.build::<C>();
        timed!(timing, "Generate proof", {
            let inner_proof = timed!(
                timing,
                "Total proof with a recursive envelope",
                plonky2::plonk::prover::prove(
                    &inner_data.prover_only,
                    &inner_data.common,
                    pw,
                    &mut timing
                )
                .unwrap()
            );
            inner_data.verify(inner_proof.clone()).unwrap();
            println!("num gates: {:?}", inner_data.common.gates.len());

            // let mut outer_builder = CircuitBuilder::<F, D>::new(CircuitConfig::standard_ecc_config());
            // let inner_proof_target = outer_builder.add_virtual_proof_with_pis(&inner_data.common);
            // let inner_verifier_data =
            //     outer_builder.add_virtual_verifier_data(inner_data.common.config.fri_config.cap_height);
            // outer_builder.verify_proof::<C>(
            //     &inner_proof_target,
            //     &inner_verifier_data,
            //     &inner_data.common,
            // );

            // let outer_data = outer_builder.build::<C>();
            // for gate in outer_data.common.gates.iter() {
            //     println!("ecddsa verify recursive gate: {:?}", gate);
            // }

            // let mut outer_pw = PartialWitness::new();
            // outer_pw.set_proof_with_pis_target(&inner_proof_target, &inner_proof);
            // outer_pw.set_verifier_data_target(&inner_verifier_data, &inner_data.verifier_only);

            // let outer_proof = outer_data.prove(outer_pw).unwrap();

            // outer_data
            //     .verify(outer_proof)
            //     .expect("failed to verify proof");
        });

        timing.print();
    }

    #[test]
    fn test_step_with_dummy_sigs() {
        // Testing block 11105 (4 validators, 2 signed)
        // Need to handle empty validators as well
        // Should set some dummy values
        let block = 11105;

        const VALIDATOR_SET_SIZE_MAX: usize = 4;

        test_step_template::<VALIDATOR_SET_SIZE_MAX>(block);
    }

    #[test]
    fn test_step() {
        // Testing block 11000
        let block = 11000;

        const VALIDATOR_SET_SIZE_MAX: usize = 4;

        test_step_template::<VALIDATOR_SET_SIZE_MAX>(block);
    }

    #[test]
    fn test_step_with_empty() {
        // Testing block 10000
        let block = 10000;

        const VALIDATOR_SET_SIZE_MAX: usize = 4;

        test_step_template::<VALIDATOR_SET_SIZE_MAX>(block);
    }

    #[test]
    fn test_step_large() {
        // Testing block 75000
        // 77 validators (128)
        // Block 50000
        // 32 validators
        // Block 15000
        // 16 validators
        // Testing block 60000
        // 60 validators, 4 disabled (valhash)

        let block = 60000;

        const VALIDATOR_SET_SIZE_MAX: usize = 64;

        test_step_template::<VALIDATOR_SET_SIZE_MAX>(block);
    }
}
