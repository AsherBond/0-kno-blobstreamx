//! The protobuf encoding of a Tendermint validator is a deterministic function of the validator's
//! public key (32 bytes) and voting power (int64). The encoding is as follows in bytes:
//
//!     10 34 10 32 <pubkey> 16 <varint>
//
//! The `pubkey` is encoded as the raw list of bytes used in the public key. The `varint` is
//! encoded using protobuf's default integer encoding, which consist of 7 bit payloads. You can
//! read more about them here: https://protobuf.dev/programming-guides/encoding/#varints.
use curta::chip::hash::sha::sha256::builder_gadget::{
    CurtaBytes, SHA256Builder, SHA256BuilderGadget,
};
use curta::math::extension::cubic::parameters::CubicParameters;
use plonky2::field::extension::Extendable;
use plonky2::iop::target::BoolTarget;
use plonky2::iop::target::Target;
use plonky2::plonk::config::{AlgebraicHasher, GenericConfig};
use plonky2::{hash::hash_types::RichField, plonk::circuit_builder::CircuitBuilder};
use plonky2x::frontend::ecc::ed25519::curve::curve_types::Curve;
use plonky2x::frontend::ecc::ed25519::curve::ed25519::Ed25519;
use plonky2x::frontend::ecc::ed25519::gadgets::curve::{AffinePointTarget, CircuitBuilderCurve};
use plonky2x::frontend::hash::sha::sha256::pad_single_sha256_chunk;
use plonky2x::frontend::num::u32::gadgets::arithmetic_u32::{CircuitBuilderU32, U32Target};
use tendermint::merkle::HASH_SIZE;

use crate::utils::{
    I64Target, MarshalledValidatorTarget, TendermintHashTarget, HASH_SIZE_BITS,
    VALIDATOR_BIT_LENGTH_MAX, VALIDATOR_BYTE_LENGTH_MAX, VOTING_POWER_BITS_LENGTH_MAX,
    VOTING_POWER_BYTES_LENGTH_MAX,
};

pub trait CelestiaCommitment<F: RichField + Extendable<D>, const D: usize> {
    type Curve: Curve;

    /// Encodes the data hash and height into a tuple.
    fn encode_data_root_tuple(
        &mut self,
        data_hash: &TendermintHashTarget,
        height: &U32Target,
    ) -> [BoolTarget; HASH_SIZE_BITS * 2];

    /// Verify a merkle proof against the specified root hash.
    /// Note: This function will only work for leaves with a length of 34 bytes (protobuf-encoded SHA256 hash)
    /// Output is the merkle root
    fn get_root_from_merkle_proof<E: CubicParameters<F>, const PROOF_DEPTH: usize>(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        aunts: &Vec<TendermintHashTarget>,
        path_indices: &Vec<BoolTarget>,
        leaf_hash: &TendermintHashTarget,
    ) -> TendermintHashTarget;

    /// Hashes leaf bytes to get the leaf hash according to the Tendermint spec. (0x00 || leafBytes)
    /// Note: Uses STARK gadget to generate SHA's.
    /// LEAF_SIZE_BITS_PLUS_8 is the number of bits in the protobuf-encoded leaf bytes.
    fn leaf_hash_stark<
        E: CubicParameters<F>,
        const LEAF_SIZE_BITS: usize,
        const LEAF_SIZE_BITS_PLUS_8: usize,
        const NUM_BYTES: usize,
    >(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        leaf: &[BoolTarget; LEAF_SIZE_BITS],
    ) -> TendermintHashTarget;

    /// Hashes two nodes to get the inner node according to the Tendermint spec. (0x01 || left || right)
    fn inner_hash_stark<E: CubicParameters<F>>(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        left: &TendermintHashTarget,
        right: &TendermintHashTarget,
    ) -> TendermintHashTarget;

    /// Hashes a layer of the Merkle tree according to the Tendermint spec. (0x01 || left || right)
    /// If in a pair the right node is not enabled (empty), then the left node is passed up to the next layer.
    /// If neither the left nor right node in a pair is enabled (empty), then the parent node is set to not enabled (empty).
    fn hash_merkle_layer<E: CubicParameters<F>>(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        merkle_hashes: &mut Vec<TendermintHashTarget>,
        merkle_hash_enabled: &mut Vec<BoolTarget>,
        num_hashes: usize,
    ) -> (Vec<TendermintHashTarget>, Vec<BoolTarget>);

    // Convert from [BoolTarget; N * 8] to SHA-256 padded CurtaBytes<N>
    fn convert_to_padded_curta_bytes<const NUM_BITS: usize, const NUM_BYTES: usize>(
        &mut self,
        bits: &[BoolTarget; NUM_BITS],
    ) -> CurtaBytes<NUM_BYTES>;

    // Convert from SHA-256 output hash CurtaBytes<HASH_SIZE> to [BoolTarget; HASH_SIZE_BITS]
    fn convert_from_curta_bytes(
        &mut self,
        curta_bytes: &CurtaBytes<HASH_SIZE>,
    ) -> TendermintHashTarget;

    /// Compute the data commitment from the data hashes and block heights. WINDOW_RANGE is the number of blocks in the data commitment. NUM_LEAVES is the number of leaves in the tree for the data commitment.
    fn get_data_commitment<
        E: CubicParameters<F>,
        C: GenericConfig<D, F = F, FE = F::Extension> + 'static,
        const WINDOW_RANGE: usize,
        const NUM_LEAVES: usize,
    >(
        &mut self,
        data_hashes: &Vec<TendermintHashTarget>,
        block_heights: &Vec<U32Target>,
    ) -> TendermintHashTarget
    where
        <C as GenericConfig<D>>::Hasher: AlgebraicHasher<F>;
}

impl<F: RichField + Extendable<D>, const D: usize> CelestiaCommitment<F, D>
    for CircuitBuilder<F, D>
{
    type Curve = Ed25519;

    fn encode_data_root_tuple(
        &mut self,
        data_hash: &TendermintHashTarget,
        height: &U32Target,
    ) -> [BoolTarget; HASH_SIZE_BITS * 2] {
        let mut data_root_tuple = [self._false(); HASH_SIZE_BITS * 2];

        // Encode the data hash.
        for i in 0..HASH_SIZE_BITS {
            data_root_tuple[i] = data_hash.0[i];
        }

        // Encode the height.
        let mut height_bits = self.u32_to_bits_le(*height);
        height_bits.reverse();
        for i in 0..32 {
            data_root_tuple[HASH_SIZE_BITS * 2 - 32 + i] = height_bits[i];
        }

        data_root_tuple
    }

    fn get_root_from_merkle_proof<E: CubicParameters<F>, const PROOF_DEPTH: usize>(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        aunts: &Vec<TendermintHashTarget>,
        // TODO: Should we hard-code path_indices to correspond to dataHash, validatorsHash and nextValidatorsHash?
        path_indices: &Vec<BoolTarget>,
        // This leaf should already be hashed. (0x00 || leafBytes)
        leaf_hash: &TendermintHashTarget,
    ) -> TendermintHashTarget {
        let mut hash_so_far = *leaf_hash;
        for i in 0..PROOF_DEPTH {
            let aunt = aunts[i];
            let path_index = path_indices[i];
            let left_hash_pair = self.inner_hash_stark::<E>(gadget, &hash_so_far, &aunt);
            let right_hash_pair = self.inner_hash_stark::<E>(gadget, &aunt, &hash_so_far);

            let mut hash_pair = [self._false(); HASH_SIZE_BITS];
            for j in 0..HASH_SIZE_BITS {
                // If the path index is 0, then the right hash is the aunt.
                hash_pair[j] = BoolTarget::new_unsafe(self.select(
                    path_index,
                    right_hash_pair.0[j].target,
                    left_hash_pair.0[j].target,
                ));
            }
            hash_so_far = TendermintHashTarget(hash_pair);
        }
        hash_so_far
    }

    fn leaf_hash_stark<
        E: CubicParameters<F>,
        const LEAF_SIZE_BITS: usize,
        const LEAF_SIZE_BITS_PLUS_8: usize,
        const NUM_BYTES: usize,
    >(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        leaf: &[BoolTarget; LEAF_SIZE_BITS],
    ) -> TendermintHashTarget {
        // NUM_BYTES must be a multiple of 32
        assert_eq!(NUM_BYTES % 64, 0);

        // Calculate the message for the leaf hash.
        let mut leaf_msg_bits = [self._false(); LEAF_SIZE_BITS_PLUS_8];

        // 0x00
        for k in 0..8 {
            leaf_msg_bits[k] = self._false();
        }

        // validatorBytes
        for k in 8..LEAF_SIZE_BITS_PLUS_8 {
            leaf_msg_bits[k] = leaf[k - 8];
        }

        // Convert the [BoolTarget; N] into Curta bytes
        let leaf_msg_bytes =
            self.convert_to_padded_curta_bytes::<LEAF_SIZE_BITS_PLUS_8, NUM_BYTES>(&leaf_msg_bits);

        // Load the output of the hash.
        let hash = self.sha256(&leaf_msg_bytes, gadget);

        self.convert_from_curta_bytes(&hash)
    }

    fn inner_hash_stark<E: CubicParameters<F>>(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        left: &TendermintHashTarget,
        right: &TendermintHashTarget,
    ) -> TendermintHashTarget {
        // Calculate the length of the message for the inner hash.
        // 0x01 || left || right
        const MSG_BITS_LENGTH: usize = 8 + (HASH_SIZE_BITS * 2);

        // Calculate the message for the inner hash.
        let mut message_bits = [self._false(); MSG_BITS_LENGTH];

        // 0x01
        for k in 0..7 {
            message_bits[k] = self._false();
        }
        message_bits[7] = self._true();

        // left
        for k in 8..8 + HASH_SIZE_BITS {
            message_bits[k] = left.0[k - 8];
        }

        // right
        for k in 8 + HASH_SIZE_BITS..MSG_BITS_LENGTH {
            message_bits[k] = right.0[k - (8 + HASH_SIZE_BITS)];
        }

        const SHA256_PADDED_NUM_BYTES: usize = 128;
        // Convert the [BoolTarget; N] into Curta bytes which requires a padded length of 128 bytes because the message is 65 bytes.
        let leaf_msg_bytes = self
            .convert_to_padded_curta_bytes::<MSG_BITS_LENGTH, SHA256_PADDED_NUM_BYTES>(
                &message_bits,
            );

        // Load the output of the hash.
        // Note: Calculate the inner hash as if both validators are enabled.
        let inner_hash = self.sha256(&leaf_msg_bytes, gadget);

        self.convert_from_curta_bytes(&inner_hash)
    }

    fn hash_merkle_layer<E: CubicParameters<F>>(
        &mut self,
        gadget: &mut SHA256BuilderGadget<F, E, D>,
        merkle_hashes: &mut Vec<TendermintHashTarget>,
        merkle_hash_enabled: &mut Vec<BoolTarget>,
        num_hashes: usize,
    ) -> (Vec<TendermintHashTarget>, Vec<BoolTarget>) {
        let zero = self.zero();
        let one = self.one();

        for i in (0..num_hashes).step_by(2) {
            let both_nodes_enabled = self.and(merkle_hash_enabled[i], merkle_hash_enabled[i + 1]);

            let first_node_disabled = self.not(merkle_hash_enabled[i]);
            let second_node_disabled = self.not(merkle_hash_enabled[i + 1]);
            let both_nodes_disabled = self.and(first_node_disabled, second_node_disabled);

            // Calculuate the inner hash.
            let inner_hash = self.inner_hash_stark::<E>(
                gadget,
                &TendermintHashTarget(merkle_hashes[i].0),
                &TendermintHashTarget(merkle_hashes[i + 1].0),
            );

            for k in 0..HASH_SIZE_BITS {
                // If the left node is enabled and the right node is disabled, we pass up the left hash instead of the inner hash.
                merkle_hashes[i / 2].0[k] = BoolTarget::new_unsafe(self.select(
                    both_nodes_enabled,
                    inner_hash.0[k].target,
                    merkle_hashes[i].0[k].target,
                ));
            }

            // Set the inner node one level up to disabled if both nodes are disabled.
            merkle_hash_enabled[i / 2] =
                BoolTarget::new_unsafe(self.select(both_nodes_disabled, zero, one));
        }

        // Return the hashes and enabled nodes for the next layer up.
        (merkle_hashes.to_vec(), merkle_hash_enabled.to_vec())
    }

    fn convert_to_padded_curta_bytes<const MSG_SIZE_BITS: usize, const NUM_BYTES: usize>(
        &mut self,
        bits: &[BoolTarget; MSG_SIZE_BITS],
    ) -> CurtaBytes<NUM_BYTES> {
        let zero = self.zero();

        let bytes = CurtaBytes(self.add_virtual_target_arr::<NUM_BYTES>());
        for i in (0..MSG_SIZE_BITS).step_by(8) {
            let mut byte = self.zero();
            for j in 0..8 {
                let bit = bits[i + j];
                // MSB first
                byte = self.mul_const_add(F::from_canonical_u8(1 << (7 - j)), bit.target, byte);
            }
            self.connect(byte, bytes.0[i / 8]);
            // bytes.0[i / 8] = byte;
        }

        // Push padding byte (0x80)
        let padding_byte = self.constant(F::from_canonical_u64(0x80));
        self.connect(padding_byte, bytes.0[MSG_SIZE_BITS / 8]);

        // Reserve 8 bytes for the length of the message.
        for i in ((MSG_SIZE_BITS + 8) / 8)..NUM_BYTES - 8 {
            // Fill the rest of the bits with zero's
            self.connect(zero, bytes.0[i]);
        }

        // Set the length bits to the length of the message.
        let len = ((MSG_SIZE_BITS) as u64).to_be_bytes();
        for i in 0..8 {
            let bit = self.constant(F::from_canonical_u8(len[i]));
            self.connect(bit, bytes.0[NUM_BYTES - 8 + i]);
        }
        bytes
    }

    fn convert_from_curta_bytes(
        &mut self,
        curta_bytes: &CurtaBytes<HASH_SIZE>,
    ) -> TendermintHashTarget {
        // Convert the Curta bytes into [BoolTarget; N]
        let mut return_hash = [self._false(); HASH_SIZE_BITS];
        for i in 0..HASH_SIZE {
            // Decompose each byte into LE bits
            let mut bits = self.split_le(curta_bytes.0[i], 8);

            // Flip to BE bits
            bits.reverse();

            // Store in return hash
            for j in 0..8 {
                return_hash[i * 8 + j] = bits[j];
            }
        }
        TendermintHashTarget(return_hash)
    }

    fn get_data_commitment<
        E: CubicParameters<F>,
        C: GenericConfig<D, F = F, FE = F::Extension> + 'static,
        const WINDOW_RANGE: usize,
        const NUM_LEAVES: usize,
    >(
        &mut self,
        data_hashes: &Vec<TendermintHashTarget>,
        block_heights: &Vec<U32Target>,
    ) -> TendermintHashTarget
    where
        <C as GenericConfig<D>>::Hasher: AlgebraicHasher<F>,
    {
        let mut gadget: SHA256BuilderGadget<F, E, D> = self.init_sha256();

        let mut leaves = vec![TendermintHashTarget([self._false(); HASH_SIZE_BITS]); NUM_LEAVES];
        let mut leaf_enabled = vec![self._false(); NUM_LEAVES];
        for i in 0..WINDOW_RANGE {
            // Encode the data hash and height into a tuple.
            let data_root_tuple = self.encode_data_root_tuple(&data_hashes[i], &block_heights[i]);

            const DATA_TUPLE_ROOT_SIZE_BITS: usize = 64 * 8;
            const DATA_TUPLE_ROOT_SIZE_BITS_PLUS_8: usize = DATA_TUPLE_ROOT_SIZE_BITS + 8;

            // Number of bytes in the padded message for SHA256.
            const PADDED_SHA256_BYTES: usize = 128;
            let leaf_hash = self
                .leaf_hash_stark::<E, DATA_TUPLE_ROOT_SIZE_BITS, DATA_TUPLE_ROOT_SIZE_BITS_PLUS_8, PADDED_SHA256_BYTES>(
                    &mut gadget,
                    &data_root_tuple,
                );
            leaves[i] = leaf_hash;
            leaf_enabled[i] = self._true();
        }

        // Fill out the first SHA256 gadget with empty leaves.
        // First chunk is 800 SHA-chunks
        // Fill out 1024 - 800 = 224 SHA-chunks
        let num_chunks_left = 224;
        fill_out_sha_gadget::<F, E, D>(self, &mut gadget, num_chunks_left);
        self.constrain_sha256_gadget::<C>(gadget);

        let mut gadget: SHA256BuilderGadget<F, E, D> = self.init_sha256();

        // Hash each of the validators to get their corresponding leaf hash.
        let mut current_nodes = leaves.clone();

        // Whether to treat the validator as empty.
        let mut current_node_enabled = leaf_enabled.clone();

        let mut merkle_layer_size = NUM_LEAVES;

        // Hash each layer of nodes to get the root according to the Tendermint spec, starting from the leaves.
        while merkle_layer_size > 1 {
            (current_nodes, current_node_enabled) = self.hash_merkle_layer(
                &mut gadget,
                &mut current_nodes,
                &mut current_node_enabled,
                merkle_layer_size,
            );
            merkle_layer_size /= 2;
        }

        // If NUM_LEAVES=512, then we have 1024 - (511 * 2) = 2 SHA-chunks left.
        // Each inner_hash_stark is 2 SHA chunks
        let num_chunks_left = 2;
        fill_out_sha_gadget::<F, E, D>(self, &mut gadget, num_chunks_left);
        self.constrain_sha256_gadget::<C>(gadget);

        // Return the root hash.
        current_nodes[0]
    }
}

fn fill_out_sha_gadget<F: RichField + Extendable<D>, E: CubicParameters<F>, const D: usize>(
    builder: &mut CircuitBuilder<F, D>,
    gadget: &mut SHA256BuilderGadget<F, E, D>,
    num_chunks_left: usize,
) {
    let zero = builder.zero();
    let bytes = CurtaBytes(builder.add_virtual_target_arr::<64>());
    for i in 0..64 {
        builder.connect(bytes.0[i], zero);
    }

    for _ in 0..num_chunks_left {
        builder.sha256(&bytes, gadget);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use curta::math::goldilocks::cubic::GoldilocksCubicParameters;
    use plonky2::{
        iop::witness::{PartialWitness, WitnessWrite},
        plonk::{
            circuit_builder::CircuitBuilder,
            circuit_data::CircuitConfig,
            config::{GenericConfig, PoseidonGoldilocksConfig},
        },
    };
    use subtle_encoding::hex;

    use crate::{
        commitment::CelestiaCommitment,
        inputs::{generate_data_commitment_inputs, get_path_indices},
        utils::{
            f_bits_to_bytes, generate_proofs_from_header, hash_all_leaves, leaf_hash, to_be_bits,
            I64Target, MarshalledValidatorTarget, TendermintHashTarget, HASH_SIZE_BITS,
            HEADER_PROOF_DEPTH, PROTOBUF_BLOCK_ID_SIZE_BITS, PROTOBUF_HASH_SIZE_BITS,
            VALIDATOR_BIT_LENGTH_MAX,
        },
    };

    type C = PoseidonGoldilocksConfig;
    type F = <C as GenericConfig<D>>::F;
    type E = GoldilocksCubicParameters;
    type Curve = Ed25519;
    const D: usize = 2;

    const WINDOW_SIZE: usize = 400;
    const NUM_LEAVES: usize = 512;

    #[test]
    fn test_data_commitment() {
        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        const START_BLOCK: usize = 3800;
        const END_BLOCK: usize = START_BLOCK + WINDOW_SIZE;

        let inputs = generate_data_commitment_inputs(START_BLOCK, END_BLOCK);

        let mut data_hashes_targets = Vec::new();
        let mut block_heights_targets = Vec::new();
        for i in 0..WINDOW_SIZE {
            let mut data_hash_target = TendermintHashTarget([builder._false(); HASH_SIZE_BITS]);

            let data_hash_bits = to_be_bits(inputs.data_hashes[i].into());
            for j in 0..HASH_SIZE_BITS {
                data_hash_target.0[j] = builder.constant_bool(data_hash_bits[j]);
            }

            let block_height = builder.constant_u32((START_BLOCK + i) as u32);

            data_hashes_targets.push(data_hash_target);
            block_heights_targets.push(block_height);
        }

        let root_hash_target = builder.get_data_commitment::<E, C, WINDOW_SIZE, NUM_LEAVES>(
            &data_hashes_targets,
            &block_heights_targets,
        );

        println!(
            "Expected data commitment root: {:?}",
            String::from_utf8(hex::encode(inputs.data_commitment_root)).unwrap()
        );

        let expected_data_commitment_bits = to_be_bits(inputs.data_commitment_root.into());

        println!(
            "Expected data commitment root bits: {:?}",
            expected_data_commitment_bits
        );

        for i in 0..HASH_SIZE_BITS {
            pw.set_bool_target(root_hash_target.0[i], expected_data_commitment_bits[i]);
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_encode_data_root_tuple() {
        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        let mut expected_data_tuple_root = vec![
            255u8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ];

        let expected_height = [
            0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 1, 0,
        ];

        expected_data_tuple_root.extend_from_slice(&expected_height);

        let expected_data_tuple_root_bits = to_be_bits(expected_data_tuple_root);

        let data_hash = TendermintHashTarget([builder._true(); HASH_SIZE_BITS]);
        let height = builder.constant_u32(256);
        let data_root_tuple = builder.encode_data_root_tuple(&data_hash, &height);

        // Check that the data hash is encoded correctly.
        for i in 0..HASH_SIZE_BITS {
            pw.set_bool_target(data_root_tuple[i], expected_data_tuple_root_bits[i])
        }

        // Check that the height is encoded correctly.
        // let height_bits = builder.u32_to_bits_le(height);
        for i in HASH_SIZE_BITS..HASH_SIZE_BITS * 2 {
            pw.set_bool_target(data_root_tuple[i], expected_data_tuple_root_bits[i])
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        data.verify(proof).unwrap();

        println!("Verified proof");
    }
}