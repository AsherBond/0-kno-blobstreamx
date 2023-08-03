//! The protobuf encoding of a Tendermint validator is a deterministic function of the validator's
//! public key (32 bytes) and voting power (int64). The encoding is as follows in bytes:
//
//!     10 34 10 32 <pubkey> 16 <varint>
//
//! The `pubkey` is encoded as the raw list of bytes used in the public key. The `varint` is
//! encoded using protobuf's default integer encoding, which consist of 7 bit payloads. You can
//! read more about them here: https://protobuf.dev/programming-guides/encoding/#varints.
use plonky2::field::extension::Extendable;
use plonky2::iop::target::{BoolTarget, Target};
use plonky2::{hash::hash_types::RichField, plonk::circuit_builder::CircuitBuilder};
use plonky2_gadgets::hash::sha::sha256::sha256;
use plonky2_gadgets::num::u32::gadgets::arithmetic_u32::{CircuitBuilderU32, U32Target};

use tendermint::merkle::HASH_SIZE;

/// The number of bytes in a SHA256 hash.
pub const HASH_SIZE_BITS: usize = HASH_SIZE * 8;

/// The number of bytes in a protobuf-encoded SHA256 hash.
pub const PROTOBUF_HASH_SIZE_BITS: usize = HASH_SIZE_BITS + 8 * 2;

/// The maximum length of a protobuf-encoded Tendermint validator in bytes.
const VALIDATOR_BYTE_LENGTH_MAX: usize = 46;

/// The maximum length of a protobuf-encoded Tendermint validator in bits.
const VALIDATOR_BIT_LENGTH_MAX: usize = VALIDATOR_BYTE_LENGTH_MAX * 8;

/// The minimum length of a protobuf-encoded Tendermint validator in bytes.
const VALIDATOR_BYTE_LENGTH_MIN: usize = 38;

/// The minimum length of a protobuf-encoded Tendermint validator in bits.
const VALIDATOR_BIT_LENGTH_MIN: usize = VALIDATOR_BYTE_LENGTH_MIN * 8;

/// The number of possible byte lengths of a protobuf-encoded Tendermint validator.
const NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS: usize =
    VALIDATOR_BYTE_LENGTH_MAX - VALIDATOR_BYTE_LENGTH_MIN + 1;

// The number of bytes in a Tendermint validator's public key.
const PUBKEY_BYTES_LEN: usize = 32;

// The maximum number of bytes in a Tendermint validator's voting power.
// https://docs.tendermint.com/v0.34/tendermint-core/using-tendermint.html#tendermint-networks
const VOTING_POWER_BYTES_LENGTH_MAX: usize = 9;

// The maximum number of bits in a Tendermint validator's voting power.
const VOTING_POWER_BITS_LENGTH_MAX: usize = VOTING_POWER_BYTES_LENGTH_MAX * 8;

// The maximum number of validators in a Tendermint validator set.
const VALIDATOR_SET_SIZE_MAX: usize = 4;

// The maximum number of bytes in a validator message (CanonicalVote toSignBytes).
const VALIDATOR_MESSAGE_BYTES_LENGTH_MAX: usize = 124;

/// The Ed25519 public key as a list of 32 byte targets.
#[derive(Debug, Clone, Copy)]
pub struct Ed25519PubkeyTarget(pub [BoolTarget; 256]);

/// The Tendermint hash as a 32 byte target.
#[derive(Debug, Clone, Copy)]
pub struct TendermintHashTarget(pub [Target; HASH_SIZE]);

/// The voting power as a list of 2 u32 targets.
#[derive(Debug, Clone, Copy)]
pub struct I64Target(pub [U32Target; 2]);

/// The bytes, public key, and voting power targets inside of a Tendermint validator.
#[derive(Debug, Clone)]
pub struct TendermintValidator {
    pub pubkey: Ed25519PubkeyTarget,
    pub voting_power: I64Target,
}

pub trait TendermintMarshaller {
    /// Serializes an int64 as a protobuf varint.
    fn marshal_int64_varint(
        &mut self,
        num: I64Target,
    ) -> [BoolTarget; VOTING_POWER_BITS_LENGTH_MAX];

    /// Serializes the validator public key and voting power to bytes.
    fn marshal_tendermint_validator(
        &mut self,
        pubkey: Ed25519PubkeyTarget,
        voting_power: I64Target,
    ) -> [BoolTarget; VALIDATOR_BIT_LENGTH_MAX];

    /// Extract the header hash from the signed message from a validator.
    fn verify_hash_in_message(
        &mut self,
        message: [BoolTarget; VALIDATOR_MESSAGE_BYTES_LENGTH_MAX * 8],
        header_hash: [BoolTarget; HASH_SIZE_BITS],
        // Should be the same for all validators
        round_present_in_message: BoolTarget,
    ) -> [BoolTarget; HASH_SIZE_BITS];

    /// Verify a merkle proof against the specified root hash.
    /// Note: This function will only work for leaves with a length of 34 bytes (protobuf-encoded SHA256 hash)
    /// Output is the merkle root
    fn get_root_from_merkle_proof(
        &mut self,
        aunts: Vec<[BoolTarget; HASH_SIZE_BITS]>,
        merkle_proof_enabled: Vec<BoolTarget>,
        leaf: [BoolTarget; PROTOBUF_HASH_SIZE_BITS],
    ) -> [BoolTarget; HASH_SIZE_BITS];

    /// Hashes leaf bytes to get the leaf hash according to the Tendermint spec. (0x00 || leafBytes)
    /// Note: This function will only work for leaves with a length of 34 bytes (protobuf-encoded SHA256 hash)
    fn hash_header_leaf(
        &mut self,
        validator: &[BoolTarget; PROTOBUF_HASH_SIZE_BITS],
    ) -> [BoolTarget; HASH_SIZE_BITS];

    /// Hashes validator bytes to get the leaf according to the Tendermint spec. (0x00 || validatorBytes)
    fn hash_validator_leaf(
        &mut self,
        validator: &[BoolTarget; VALIDATOR_BIT_LENGTH_MAX],
        validator_byte_length: &U32Target,
    ) -> [BoolTarget; HASH_SIZE_BITS];

    /// Hashes multiple validators to get their leaves according to the Tendermint spec using hash_validator_leaf.
    fn hash_validator_leaves(
        &mut self,
        validators: &Vec<[BoolTarget; VALIDATOR_BIT_LENGTH_MAX]>,
        validator_byte_lengths: &Vec<U32Target>,
    ) -> Vec<[BoolTarget; HASH_SIZE_BITS]>;

    /// Hashes two nodes to get the inner node according to the Tendermint spec. (0x01 || left || right)
    fn inner_hash(
        &mut self,
        left: &[BoolTarget; HASH_SIZE_BITS],
        right: &[BoolTarget; HASH_SIZE_BITS],
    ) -> [BoolTarget; HASH_SIZE_BITS];

    /// Hashes a layer of the Merkle tree according to the Tendermint spec. (0x01 || left || right)
    /// If in a pair the right node is not enabled (empty), then the left node is passed up to the next layer.
    /// If neither the left nor right node in a pair is enabled (empty), then the parent node is set to not enabled (empty).
    fn hash_merkle_layer(
        &mut self,
        merkle_hashes: &mut Vec<[BoolTarget; 256]>,
        merkle_hash_enabled: &mut Vec<BoolTarget>,
        num_hashes: usize,
    ) -> (Vec<[BoolTarget; 256]>, Vec<BoolTarget>);

    /// Compute the expected validator hash from the validator set.
    fn hash_validator_set(
        &mut self,
        validators: &Vec<[BoolTarget; VALIDATOR_BIT_LENGTH_MAX]>,
        validator_byte_length: &Vec<U32Target>,
        validator_enabled: &Vec<BoolTarget>,
    ) -> [BoolTarget; HASH_SIZE * 8];

    fn mul_i64_by_u32(&mut self, a: &I64Target, b: U32Target) -> I64Target;

    // Returns a >= b
    fn is_i64_gte(&mut self, a: &I64Target, b: &I64Target) -> BoolTarget;

    // Gets the total voting power by summing the voting power of all validators.
    fn get_total_voting_power(&mut self, validator_voting_power: &Vec<I64Target>) -> I64Target;

    // Checks if accumulated voting power * m > total voting power * n (threshold is n/m)
    fn voting_power_greater_than_threshold(
        &mut self,
        accumulated_power: &I64Target,
        total_voting_power: &I64Target,
        threshold_numerator: U32Target,
        threshold_denominator: U32Target,
    ) -> BoolTarget;

    /// Accumulate voting power from the enabled validators & check that the voting power is greater than 2/3 of the total voting power.
    fn check_voting_power(
        &mut self,
        validator_voting_power: &Vec<I64Target>,
        validator_enabled: &Vec<U32Target>,
        total_voting_power: &I64Target,
        threshold_numerator: U32Target,
        threshold_denominator: U32Target,
    ) -> BoolTarget;
}

impl<F: RichField + Extendable<D>, const D: usize> TendermintMarshaller for CircuitBuilder<F, D> {
    fn get_root_from_merkle_proof(
        &mut self,
        aunts: Vec<[BoolTarget; HASH_SIZE_BITS]>,
        path_indices: Vec<BoolTarget>,
        leaf: [BoolTarget; PROTOBUF_HASH_SIZE_BITS],
    ) -> [BoolTarget; HASH_SIZE_BITS] {
        let hash_leaf = self.hash_header_leaf(&leaf);

        let mut hash_so_far = hash_leaf;
        for i in 0..aunts.len() {
            let aunt = aunts[i];
            let path_index = path_indices[i];
            let left_hash_pair = self.inner_hash(&hash_so_far, &aunt);
            let right_hash_pair = self.inner_hash(&aunt, &hash_so_far);

            let mut hash_pair = [self._false(); HASH_SIZE_BITS];
            for j in 0..HASH_SIZE_BITS {
                // If the path index is 0, then the right hash is the aunt.
                hash_pair[j] = BoolTarget::new_unsafe(self.select(
                    path_index,
                    right_hash_pair[j].target,
                    left_hash_pair[j].target,
                ));
            }
            hash_so_far = hash_pair;
        }
        hash_so_far
    }

    fn hash_header_leaf(
        &mut self,
        leaf: &[BoolTarget; PROTOBUF_HASH_SIZE_BITS],
    ) -> [BoolTarget; HASH_SIZE_BITS] {
        // Calculate the length of the message for the leaf hash.
        // 0x00 || leafBytes
        let bits_length = 8 + (PROTOBUF_HASH_SIZE_BITS);

        // Calculate the message for the leaf hash.
        let mut leaf_msg_bits = vec![self._false(); bits_length];

        // 0x00
        for k in 0..8 {
            leaf_msg_bits[k] = self._false();
        }

        // validatorBytes
        for k in 8..bits_length {
            leaf_msg_bits[k] = leaf[k - 8];
        }

        // Load the output of the hash.
        let hash = sha256(self, &leaf_msg_bits);
        let mut return_hash = [self._false(); HASH_SIZE_BITS];
        for k in 0..HASH_SIZE_BITS {
            return_hash[k] = hash[k];
        }
        return_hash
    }

    fn verify_hash_in_message(
        &mut self,
        message: [BoolTarget; VALIDATOR_MESSAGE_BYTES_LENGTH_MAX * 8],
        header_hash: [BoolTarget; HASH_SIZE_BITS],
        // Should be the same for all validators
        round_present_in_message: BoolTarget,
    ) -> [BoolTarget; HASH_SIZE_BITS] {
        // Logic:
        //      Verify that header_hash is equal to the hash in the message at the correct index.
        //      If the round is missing, then the hash starts at index 16.
        //      If the round is present, then the hash starts at index 25.

        let missing_round_start_idx = 16;

        let including_round_start_idx = 25;

        let one = self.one();

        let mut vec_round_missing = [self._false(); HASH_SIZE_BITS];

        let mut vec_round_present = [self._false(); HASH_SIZE_BITS];

        for i in 0..HASH_SIZE_BITS {
            vec_round_missing[i] = message[(missing_round_start_idx) * 8 + i];
            vec_round_present[i] = message[(including_round_start_idx) * 8 + i];
            let round_missing_eq =
                self.is_equal(header_hash[i].target, vec_round_missing[i].target);
            let round_present_eq =
                self.is_equal(header_hash[i].target, vec_round_present[i].target);

            // Pick the correct bit based on whether the round is present or not.
            let hash_eq = self.select(
                round_present_in_message,
                round_present_eq.target,
                round_missing_eq.target,
            );

            self.connect(hash_eq, one);
        }

        header_hash
    }

    fn marshal_int64_varint(
        &mut self,
        voting_power: I64Target,
    ) -> [BoolTarget; VOTING_POWER_BITS_LENGTH_MAX] {
        let zero = self.zero();
        let one = self.one();

        // The remaining bytes of the serialized validator are the voting power as a "varint".
        // Note: need to be careful regarding U64 and I64 differences.
        let voting_power_bits_lower = self.u32_to_bits_le(voting_power.0[0]);
        let voting_power_bits_upper = self.u32_to_bits_le(voting_power.0[1]);
        let voting_power_bits = [voting_power_bits_lower, voting_power_bits_upper].concat();

        // Check that the MSB of the voting power is zero.
        self.assert_zero(voting_power_bits[voting_power_bits.len() - 1].target);

        // The septet (7 bit) payloads  of the "varint".
        let septets = (0..VOTING_POWER_BYTES_LENGTH_MAX)
            .map(|i| {
                let mut base = F::ONE;
                let mut septet = self.zero();
                for j in 0..7 {
                    let bit = voting_power_bits[i * 7 + j];
                    septet = self.mul_const_add(base, bit.target, septet);
                    base *= F::TWO;
                }
                septet
            })
            .collect::<Vec<_>>();

        // Calculates whether the septet is not zero.
        let is_zero_septets = (0..VOTING_POWER_BYTES_LENGTH_MAX)
            .map(|i| self.is_equal(septets[i], zero).target)
            .collect::<Vec<_>>();

        // Calculates the index of the last non-zero septet.
        let mut last_seen_non_zero_septet_idx = self.zero();
        for i in 0..VOTING_POWER_BYTES_LENGTH_MAX {
            let is_nonzero_septet = self.sub(one, is_zero_septets[i]);
            let condition = BoolTarget::new_unsafe(is_nonzero_septet);
            let idx = self.constant(F::from_canonical_usize(i));
            last_seen_non_zero_septet_idx =
                self.select(condition, idx, last_seen_non_zero_septet_idx);
        }

        // If the index of a septet is elss than the last non-zero septet, set the most significant
        // bit of the byte to 1 and copy the septet bits into the lower 7 bits. Otherwise, still
        // copy the bit but the set the most significant bit to zero.
        let mut buffer = [self._false(); VOTING_POWER_BYTES_LENGTH_MAX * 8];
        for i in 0..VOTING_POWER_BYTES_LENGTH_MAX {
            // If the index is less than the last non-zero septet index, `diff` will be in
            // [0, VOTING_POWER_BYTES_LENGTH_MAX).
            let idx = self.constant(F::from_canonical_usize(i + 1));
            let diff = self.sub(last_seen_non_zero_septet_idx, idx);

            // Calculates whether we've seen at least one `diff` in [0, VOTING_POWER_BYTES_LENGTH_MAX).
            let mut is_lt_last_non_zero_septet_idx = BoolTarget::new_unsafe(zero);
            for j in 0..VOTING_POWER_BYTES_LENGTH_MAX {
                let candidate_idx = self.constant(F::from_canonical_usize(j));
                let is_candidate = self.is_equal(diff, candidate_idx);
                is_lt_last_non_zero_septet_idx =
                    self.or(is_lt_last_non_zero_septet_idx, is_candidate);
            }

            // Copy septet bits into the buffer.
            for j in 0..7 {
                let bit = voting_power_bits[i * 7 + j];
                buffer[i * 8 + j] = bit;
            }

            // Set the most significant bit of the byte to 1 if the index is less than the last
            // non-zero septet index.
            buffer[i * 8 + 7] = is_lt_last_non_zero_septet_idx;
        }

        return buffer;
    }

    fn marshal_tendermint_validator(
        &mut self,
        pubkey: Ed25519PubkeyTarget,
        voting_power: I64Target,
    ) -> [BoolTarget; VALIDATOR_BYTE_LENGTH_MAX * 8] {
        let mut ptr = 0;
        let mut buffer = [self._false(); VALIDATOR_BYTE_LENGTH_MAX * 8];

        // The first four prefix bytes of the serialized validator are `10 34 10 32`.
        let prefix_pubkey_bytes = [10, 34, 10, 32];
        for i in 0..prefix_pubkey_bytes.len() {
            for j in 0..8 {
                let bit = self.constant(F::from_canonical_u64((prefix_pubkey_bytes[i] >> j) & 1));
                buffer[ptr] = BoolTarget::new_unsafe(bit);
                ptr += 1;
            }
        }

        // The next 32 bytes of the serialized validator are the public key.
        for i in 0..PUBKEY_BYTES_LEN {
            for j in 0..8 {
                buffer[ptr] = pubkey.0[i * 8 + j];
                ptr += 1;
            }
        }

        // The next byte of the serialized validator is `16`.
        let prefix_voting_power_byte = 16;
        for j in 0..8 {
            let bit = self.constant(F::from_canonical_u64((prefix_voting_power_byte >> j) & 1));
            buffer[ptr] = BoolTarget::new_unsafe(bit);
            ptr += 1;
        }

        // The remaining bytes of the serialized validator are the voting power as a "varint".
        let voting_power_bits = self.marshal_int64_varint(voting_power);
        for i in 0..VOTING_POWER_BYTES_LENGTH_MAX {
            for j in 0..8 {
                buffer[ptr] = voting_power_bits[i * 8 + j];
                ptr += 1;
            }
        }

        buffer
    }

    fn hash_validator_leaf(
        &mut self,
        validator: &[BoolTarget; VALIDATOR_BIT_LENGTH_MAX],
        validator_byte_length: &U32Target,
    ) -> [BoolTarget; HASH_SIZE_BITS] {
        // Range check the validator byte length is between [VALIDATOR_BYTE_LENGTH_MIN, VALIDATOR_BYTE_LENGTH_MAX]
        let min_validator_bytes_length =
            self.constant(F::from_canonical_usize(VALIDATOR_BYTE_LENGTH_MIN));
        let max_validator_bytes_length =
            self.constant(F::from_canonical_usize(VALIDATOR_BYTE_LENGTH_MAX));

        // len - min
        let diff_with_min_length = self.sub(validator_byte_length.0, min_validator_bytes_length);

        // max - len
        let diff_with_max_length = self.sub(max_validator_bytes_length, validator_byte_length.0);

        // Check that diff_with_min_len and diff_with_max_len are both small (if outside of range, one would be a large element of F).
        self.range_check(diff_with_min_length, 4);
        self.range_check(diff_with_max_length, 4);

        // Note: Because the byte length of each validator is variable, need to hash the validator bytes for each potential byte length.
        let mut validator_bytes_hashes =
            [[self._false(); HASH_SIZE_BITS]; NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS];
        for j in 0..NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS {
            // Calculate the length of the message for the leaf hash.
            // 0x00 || validatorBytes
            let bits_length = 8 + (VALIDATOR_BYTE_LENGTH_MIN + j) * 8;

            // Calculate the message for the leaf hash.
            let mut validator_bits = vec![self._false(); bits_length];

            // 0x00
            for k in 0..8 {
                validator_bits[k] = self._false();
            }

            // validatorBytes
            for k in 8..bits_length {
                validator_bits[k] = validator[k - 8];
            }

            // Load the output of the hash.
            let hash = sha256(self, &validator_bits);
            for k in 0..HASH_SIZE_BITS {
                validator_bytes_hashes[j][k] = hash[k];
            }
        }
        let validator_byte_length_min_constant =
            self.constant(F::from_canonical_u32(VALIDATOR_BYTE_LENGTH_MIN as u32));

        // Calculate the index of the validator's bytes length in the range [0, NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS).
        let length_index = self.sub(validator_byte_length.0, validator_byte_length_min_constant);

        // Create a bitmap, with a selector bit set to 1 if the current index corresponds to the index of the validator's bytes length.
        let mut validator_byte_hash_selector = [self._false(); NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS];
        for j in 0..NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS {
            let byte_length_index = self.constant(F::from_canonical_u32(j as u32));
            validator_byte_hash_selector[j] = self.is_equal(length_index, byte_length_index);
        }

        let mut ret_validator_leaf_hash = [self._false(); HASH_SIZE_BITS];
        for j in 0..NUM_POSSIBLE_VALIDATOR_BYTE_LENGTHS {
            for k in 0..HASH_SIZE_BITS {
                // Select the correct byte hash for the validator's byte length.
                // Copy the bits from the correct byte hash into the return hash if the selector bit for that byte length is set to 1.
                // In all other cases, keep the existing bits in the return hash, yielding desired behavior.
                ret_validator_leaf_hash[k] = BoolTarget::new_unsafe(self.select(
                    validator_byte_hash_selector[j],
                    validator_bytes_hashes[j][k].target,
                    ret_validator_leaf_hash[k].target,
                ));
            }
        }

        ret_validator_leaf_hash
    }

    fn hash_validator_leaves(
        &mut self,
        validators: &Vec<[BoolTarget; VALIDATOR_BIT_LENGTH_MAX]>,
        validator_byte_lengths: &Vec<U32Target>,
    ) -> Vec<[BoolTarget; HASH_SIZE_BITS]> {
        let num_validators = self.constant(F::from_canonical_usize(validators.len()));
        let num_validator_byte_lengths =
            self.constant(F::from_canonical_usize(validator_byte_lengths.len()));
        let validator_set_size_max = self.constant(F::from_canonical_usize(VALIDATOR_SET_SIZE_MAX));

        // Assert validators length is VALIDATOR_SET_SIZE_MAX
        self.connect(num_validators, validator_set_size_max);

        // Assert validator_byte_length length is VALIDATOR_SET_SIZE_MAX
        self.connect(num_validator_byte_lengths, validator_set_size_max);

        // For each validator
        // 1) Generate the SHA256 hash for each potential byte length of the validator from VALIDATOR_BYTE_LENGTH_MIN to VALIDATOR_BYTE_LENGTH_MAX.
        // 2) Select the hash of the correct byte length.
        // 3) Return the correct hash.

        // Hash each of the validators into a leaf hash.
        let mut validators_leaf_hashes = [[self._false(); HASH_SIZE_BITS]; VALIDATOR_SET_SIZE_MAX];
        for i in 0..VALIDATOR_SET_SIZE_MAX {
            validators_leaf_hashes[i] =
                self.hash_validator_leaf(&validators[i], &validator_byte_lengths[i]);
        }

        validators_leaf_hashes.to_vec()
    }

    fn inner_hash(
        &mut self,
        left: &[BoolTarget; HASH_SIZE_BITS],
        right: &[BoolTarget; HASH_SIZE_BITS],
    ) -> [BoolTarget; HASH_SIZE_BITS] {
        // Calculate the length of the message for the inner hash.
        // 0x01 || left || right
        let bits_length = 8 + (HASH_SIZE_BITS * 2);

        // Calculate the message for the inner hash.
        let mut message_bits = vec![self._false(); bits_length];

        // 0x01
        for k in 0..7 {
            message_bits[k] = self._false();
        }
        message_bits[7] = self._true();

        // left
        for k in 8..8 + HASH_SIZE_BITS {
            message_bits[k] = left[k - 8];
        }

        // right
        for k in 8 + HASH_SIZE_BITS..bits_length {
            message_bits[k] = right[k - (8 + HASH_SIZE_BITS)];
        }

        // Load the output of the hash.
        // Note: Calculate the inner hash as if both validators are enabled.
        let inner_hash = sha256(self, &message_bits);
        let mut ret_inner_hash = [self._false(); HASH_SIZE_BITS];
        for k in 0..HASH_SIZE_BITS {
            ret_inner_hash[k] = inner_hash[k];
        }
        ret_inner_hash
    }

    fn hash_merkle_layer(
        &mut self,
        merkle_hashes: &mut Vec<[BoolTarget; 256]>,
        merkle_hash_enabled: &mut Vec<BoolTarget>,
        num_hashes: usize,
    ) -> (Vec<[BoolTarget; 256]>, Vec<BoolTarget>) {
        let zero = self.zero();
        let one = self.one();

        for i in (0..num_hashes).step_by(2) {
            let both_nodes_enabled = self.and(merkle_hash_enabled[i], merkle_hash_enabled[i + 1]);

            let first_node_disabled = self.not(merkle_hash_enabled[i]);
            let second_node_disabled = self.not(merkle_hash_enabled[i + 1]);
            let both_nodes_disabled = self.and(first_node_disabled, second_node_disabled);

            // Calculuate the inner hash.
            let inner_hash = self.inner_hash(&merkle_hashes[i], &merkle_hashes[i + 1]);

            for k in 0..HASH_SIZE_BITS {
                // If the left node is enabled and the right node is disabled, we pass up the left hash instead of the inner hash.
                merkle_hashes[i / 2][k] = BoolTarget::new_unsafe(self.select(
                    both_nodes_enabled,
                    inner_hash[k].target,
                    merkle_hashes[i][k].target,
                ));
            }

            // Set the inner node one level up to disabled if both nodes are disabled.
            merkle_hash_enabled[i / 2] =
                BoolTarget::new_unsafe(self.select(both_nodes_disabled, zero, one));
        }

        // Return the hashes and enabled nodes for the next layer up.
        (merkle_hashes.to_vec(), merkle_hash_enabled.to_vec())
    }

    fn hash_validator_set(
        &mut self,
        validators: &Vec<[BoolTarget; VALIDATOR_BIT_LENGTH_MAX]>,
        validator_byte_lengths: &Vec<U32Target>,
        validator_enabled: &Vec<BoolTarget>,
    ) -> [BoolTarget; HASH_SIZE_BITS] {
        let num_validators = self.constant(F::from_canonical_usize(validators.len()));
        let num_validator_byte_lengths =
            self.constant(F::from_canonical_usize(validator_byte_lengths.len()));
        let num_validator_enabled = self.constant(F::from_canonical_usize(validator_enabled.len()));
        let validator_set_size_max = self.constant(F::from_canonical_usize(VALIDATOR_SET_SIZE_MAX));

        // Assert validators length is VALIDATOR_SET_SIZE_MAX
        self.connect(num_validators, validator_set_size_max);

        // Assert validator_byte_length length is VALIDATOR_SET_SIZE_MAX
        self.connect(num_validator_byte_lengths, validator_set_size_max);

        // Assert validator_enabled length is VALIDATOR_SET_SIZE_MAX
        self.connect(num_validator_enabled, validator_set_size_max);

        // Hash each of the validators to get their corresponding leaf hash.
        let mut current_validator_hashes =
            self.hash_validator_leaves(validators, validator_byte_lengths);

        // Whether to treat the validator as empty.
        let mut current_validator_enabled = validator_enabled.clone();

        let mut merkle_layer_size = VALIDATOR_SET_SIZE_MAX;

        // Hash each layer of nodes to get the root according to the Tendermint spec, starting from the leaves.
        while merkle_layer_size > 1 {
            (current_validator_hashes, current_validator_enabled) = self.hash_merkle_layer(
                &mut current_validator_hashes,
                &mut current_validator_enabled,
                merkle_layer_size,
            );
            merkle_layer_size /= 2;
        }

        // Return the root hash.
        current_validator_hashes[0]
    }

    fn mul_i64_by_u32(&mut self, a: &I64Target, b: U32Target) -> I64Target {
        // Multiply the lower 32 bits of the accumulated voting power by b
        let (lower_product, lower_carry) = self.mul_u32(a.0[0], b);

        // Multiply the upper 32 bits of the accumulated voting power by b
        let (upper_product, upper_carry) = self.mul_u32(a.0[1], b);

        // NOTE: This will limit the maximum size of numbers to (2^64 - 1) / b
        self.assert_zero_u32(upper_carry);

        // Add the carry from the lower 32 bits of the accumulated voting power to the upper 32 bits of the accumulated voting power
        let (upper_sum, upper_carry) = self.add_u32(upper_product, lower_carry);

        // Check that we did not overflow when multiplying the upper bits
        self.assert_zero_u32(upper_carry);

        I64Target([lower_product, upper_sum])
    }

    // Returns a >= b
    fn is_i64_gte(&mut self, a: &I64Target, b: &I64Target) -> BoolTarget {
        // Check that the a >= b
        // 1) a_high > b_high => TRUE
        // 2) a_high == b_high
        //  a) a_low >= b_low => TRUE
        //  b) a_low < b_low => FAIL
        // 3) a_high < b_high => FAIL

        let zero_u32 = self.constant_u32(0);

        let (result_high, underflow_high) = self.sub_u32(a.0[1], b.0[1], zero_u32);

        let no_underflow_high = self.is_equal(underflow_high.0, zero_u32.0);

        // Check if upper 32 bits are equal (a_high - b_high = 0)
        let upper_equal = self.is_equal(result_high.0, zero_u32.0);

        let upper_not_equal = self.not(upper_equal);

        // Underflows if a_low < b_low
        let (_, underflow_low) = self.sub_u32(a.0[0], b.0[0], zero_u32);

        let no_underflow_low = self.is_equal(underflow_low.0, zero_u32.0);

        // Case 1)
        // If there was no underflow & a_high - b_high is not equal (i.e. positive), accumulated voting power is greater.
        let upper_pass = self.and(upper_not_equal, no_underflow_high);

        // Case 2a)
        // If a_high = b_high & a_low >= b_low, accumulated voting power is greater.
        let lower_pass = self.and(upper_equal, no_underflow_low);

        // Note: True if accumulated voting power is >= than 2/3 of the total voting power.
        self.or(upper_pass, lower_pass)
    }

    fn get_total_voting_power(&mut self, validator_voting_power: &Vec<I64Target>) -> I64Target {
        // Sum up the voting power of all the validators

        // Get a vector of the first element of each validator's voting power using a map and collect
        let mut validator_voting_power_first = Vec::new();
        for i in 0..VALIDATOR_SET_SIZE_MAX {
            validator_voting_power_first.push(validator_voting_power[i].0[0]);
        }

        let (sum_lower_low, sum_lower_high) = self.add_many_u32(&mut validator_voting_power_first);

        let mut validator_voting_power_second = Vec::new();
        for i in 0..VALIDATOR_SET_SIZE_MAX {
            validator_voting_power_second.push(validator_voting_power[i].0[1]);
        }
        let (sum_upper_low, sum_upper_high) = self.add_many_u32(&mut validator_voting_power_second);

        self.assert_zero_u32(sum_upper_high);

        let (carry_sum_low, carry_sum_high) = self.add_u32(sum_lower_high, sum_upper_low);

        self.assert_zero_u32(carry_sum_high);

        I64Target([sum_lower_low, carry_sum_low])
    }

    fn voting_power_greater_than_threshold(
        &mut self,
        accumulated_power: &I64Target,
        total_voting_power: &I64Target,
        threshold_numerator: U32Target,
        threshold_denominator: U32Target,
    ) -> BoolTarget {
        // Threshold is numerator/denominator * total_voting_power

        // Compute accumulated_voting_power * m
        let scaled_accumulated_vp = self.mul_i64_by_u32(accumulated_power, threshold_denominator);

        // Compute total_vp * n
        let scaled_total_vp = self.mul_i64_by_u32(total_voting_power, threshold_numerator);

        self.is_i64_gte(&scaled_accumulated_vp, &scaled_total_vp)
    }

    fn check_voting_power(
        &mut self,
        validator_voting_power: &Vec<I64Target>,
        validator_enabled: &Vec<U32Target>,
        total_voting_power: &I64Target,
        threshold_numerator: U32Target,
        threshold_denominator: U32Target,
    ) -> BoolTarget {
        // Accumulate the voting power from the enabled validators.
        let mut accumulated_voting_power =
            I64Target([U32Target(self.zero()), U32Target(self.zero())]);
        for i in 0..VALIDATOR_SET_SIZE_MAX {
            let voting_power = validator_voting_power[i];
            let enabled = validator_enabled[i];

            // Note: Tendermint validators max voting power is 2^63 - 1. (Should below 2^32)
            let (sum_lower_low, sum_lower_high) =
                self.mul_add_u32(voting_power.0[0], enabled, accumulated_voting_power.0[0]);

            let (carry_sum_low, carry_sum_high) = self.add_u32(sum_lower_high, voting_power.0[1]);

            // This should not overflow from carrying voting_power[1] + accumulated_voting_power[0]
            self.assert_zero_u32(carry_sum_high);

            // This should not overflow
            let (sum_upper_low, sum_upper_high) =
                self.mul_add_u32(carry_sum_low, enabled, accumulated_voting_power.0[1]);

            // Check that the upper 32 bits of the upper sum are zero.
            self.assert_zero_u32(sum_upper_high);

            accumulated_voting_power.0[0] = sum_lower_low;
            accumulated_voting_power.0[1] = sum_upper_low;
        }

        // Note: Because the threshold is n/m, max I64 should be range checked to be < 2^63 / m
        self.voting_power_greater_than_threshold(
            &accumulated_voting_power,
            total_voting_power,
            threshold_numerator,
            threshold_denominator,
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use plonky2::field::types::Field;
    use plonky2::iop::target::BoolTarget;
    use plonky2::{
        iop::witness::{PartialWitness, WitnessWrite},
        plonk::{
            circuit_builder::CircuitBuilder,
            circuit_data::CircuitConfig,
            config::{GenericConfig, PoseidonGoldilocksConfig},
        },
    };
    use sha2::Sha256;
    use subtle_encoding::hex;
    use tendermint_proto::Protobuf;

    use crate::validator::{VALIDATOR_BIT_LENGTH_MAX, VALIDATOR_SET_SIZE_MAX};

    use crate::merkle::{generate_proofs_from_header, hash_all_leaves, leaf_hash};

    use plonky2_gadgets::num::u32::gadgets::arithmetic_u32::U32Target;

    use crate::{
        utils::{bits_to_bytes, f_bits_to_bytes},
        validator::{I64Target, TendermintMarshaller},
    };

    use super::{Ed25519PubkeyTarget, VALIDATOR_BYTE_LENGTH_MIN};

    type C = PoseidonGoldilocksConfig;
    type F = <C as GenericConfig<D>>::F;
    const D: usize = 2;

    fn to_bits(msg: Vec<u8>) -> Vec<bool> {
        let mut res = Vec::new();
        for i in 0..msg.len() {
            let char = msg[i];
            for j in 0..8 {
                if (char & (1 << 7 - j)) != 0 {
                    res.push(true);
                } else {
                    res.push(false);
                }
            }
        }
        res
    }

    // Generate the inputs from the validator byte arrays.
    fn generate_inputs(
        builder: &mut CircuitBuilder<F, D>,
        validators: &Vec<&str>,
    ) -> (
        Vec<[BoolTarget; VALIDATOR_BIT_LENGTH_MAX]>,
        Vec<U32Target>,
        Vec<BoolTarget>,
    ) {
        let mut validator_byte_length: Vec<U32Target> =
            vec![
                U32Target(builder.constant(F::from_canonical_usize(VALIDATOR_BYTE_LENGTH_MIN)));
                VALIDATOR_SET_SIZE_MAX
            ];

        let mut validator_enabled: Vec<BoolTarget> = vec![builder._false(); VALIDATOR_SET_SIZE_MAX];

        let mut validator_bits: Vec<Vec<bool>> = (0..256).map(|_| Vec::<bool>::new()).collect();

        let mut validators_target: Vec<[BoolTarget; VALIDATOR_BIT_LENGTH_MAX]> =
            vec![[builder._false(); VALIDATOR_BIT_LENGTH_MAX]; VALIDATOR_SET_SIZE_MAX];

        // Convert the hex strings to bytes.
        for i in 0..validators.len() {
            let val_byte_length = validators[i].len() / 2;
            validator_bits[i] = to_bits(hex::decode(validators[i]).unwrap());
            for j in 0..(val_byte_length * 8) {
                if validator_bits[i][j] {
                    validators_target[i][j] = builder._true();
                } else {
                    validators_target[i][j] = builder._false();
                }
            }
            validator_byte_length[i] =
                U32Target(builder.constant(F::from_canonical_usize(val_byte_length)));
            validator_enabled[i] = builder._true();
        }
        return (validators_target, validator_byte_length, validator_enabled);
    }

    #[test]
    fn test_hash_header_leaf() {
        let block = tendermint::Block::from(
            serde_json::from_str::<tendermint::block::Block>(include_str!(
                "./fixtures/celestia_block.json"
            ))
            .unwrap(),
        );

        let encoded_validators_hash_bits = to_bits(block.header.validators_hash.encode_vec());
        // Note: Make sure to encode_vec()
        let validators_leaf_hash =
            leaf_hash::<Sha256>(&block.header.validators_hash.encode_vec()).to_vec();

        let validators_hash_bits = to_bits(validators_leaf_hash);

        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        let mut validators_hash_bits_target = [builder._false(); PROTOBUF_HASH_SIZE_BITS];
        for i in 0..encoded_validators_hash_bits.len() {
            if encoded_validators_hash_bits[i] {
                validators_hash_bits_target[i] = builder._true();
            }
        }

        let result = builder.hash_header_leaf(&validators_hash_bits_target);

        for i in 0..HASH_SIZE_BITS {
            if validators_hash_bits[i] {
                pw.set_target(result[i].target, F::ONE);
            } else {
                pw.set_target(result[i].target, F::ZERO);
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        println!("Created proof");

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_get_root_from_merkle_proof() {
        // Generate test cases from Celestia block:
        let block = tendermint::Block::from(
            serde_json::from_str::<tendermint::block::Block>(include_str!(
                "./fixtures/celestia_block.json"
            ))
            .unwrap(),
        );

        let header_hash = block.header.hash().to_string();
        let header_bits = to_bits(hex::decode(header_hash.to_lowercase()).unwrap());

        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        let (root_hash, proofs) = generate_proofs_from_header(&block.header);

        println!("root_hash: {:?}", String::from_utf8(hex::encode(root_hash)));

        // Can test with leaf_index 6, 7 or 8 (data_hash, validators_hash, next_validators_hash)
        let leaf_index = 8;

        // Note: Make sure to encode_vec()
        // let leaf = block.header.data_hash.expect("data hash present").encode_vec();
        // let leaf = block.header.validators_hash.encode_vec();
        let leaf = block.header.next_validators_hash.encode_vec();

        println!(
            "encoded leaf: {:?}",
            String::from_utf8(hex::encode(leaf.clone()))
        );
        let leaf_bits = to_bits(leaf);

        let mut path_indices = vec![];

        let mut current_total = proofs[leaf_index].total as usize;
        let mut current_index = leaf_index as usize;
        while current_total >= 1 {
            path_indices.push(builder.constant_bool(current_index % 2 == 1));
            current_total = current_total / 2;
            current_index = current_index / 2;
        }

        let mut leaf_target = [builder._false(); PROTOBUF_HASH_SIZE_BITS];
        for i in 0..PROTOBUF_HASH_SIZE_BITS {
            leaf_target[i] = if leaf_bits[i] {
                builder._true()
            } else {
                builder._false()
            };
        }

        let mut aunts_target =
            vec![[builder._false(); HASH_SIZE_BITS]; proofs[leaf_index].aunts.len()];
        for i in 0..proofs[leaf_index].aunts.len() {
            let bool_vector = to_bits(proofs[leaf_index].aunts[i].to_vec());

            for j in 0..HASH_SIZE_BITS {
                aunts_target[i][j] = if bool_vector[j] {
                    builder._true()
                } else {
                    builder._false()
                };
            }
        }

        let result = builder.get_root_from_merkle_proof(aunts_target, path_indices, leaf_target);

        for i in 0..HASH_SIZE_BITS {
            if header_bits[i] {
                pw.set_target(result[i].target, F::ONE);
            } else {
                pw.set_target(result[i].target, F::ZERO);
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        println!("Created proof");

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_get_leaf_hash() {
        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        // Computed the leaf hashes corresponding to the first validator bytes. SHA256(0x00 || validatorBytes)
        let expected_digest = "84f633a570a987326947aafd434ae37f151e98d5e6d429137a4cc378d4a7988e";
        let digest_bits = to_bits(hex::decode(expected_digest).unwrap());

        let validators: Vec<&str> = vec![
            "de6ad0941095ada2a7996e6a888581928203b8b69e07ee254d289f5b9c9caea193c2ab01902d",
            "92fbe0c52937d80c5ea643c7832620b84bfdf154ec7129b8b471a63a763f2fe955af1ac65fd3",
            "e902f88b2371ff6243bf4b0ebe8f46205e00749dd4dad07b2ea34350a1f9ceedb7620ab913c2",
        ];

        let (validators_target, validator_byte_length, _) =
            generate_inputs(&mut builder, &validators);

        let result = builder.hash_validator_leaf(&validators_target[0], &validator_byte_length[0]);

        // Set the target bits to the expected digest bits.
        for i in 0..HASH_SIZE_BITS {
            if digest_bits[i] {
                pw.set_target(result[i].target, F::ONE);
            } else {
                pw.set_target(result[i].target, F::ZERO);
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_hash_validator_leaves() {
        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        let validators: Vec<&str> = vec!["6694200ba0e084f7184255abedc39af04463a4ff11e0e0c1326b1b82ea1de50c6b35cf6efa8f7ed3", "739d312e54353379a852b43de497ca4ec52bb49f59b7294a4d6cf19dd648e16cb530b7a7a1e35875d4ab4d90", "4277f2f871f3e041bcd4643c0cf18e5a931c2bfe121ce8983329a289a2b0d2161745a2ddf99bade9a1"];

        let validators_bytes: Vec<Vec<u8>> = validators
            .iter()
            .map(|x| hex::decode(x).unwrap())
            .collect::<Vec<_>>();

        let expected_digests_bytes = hash_all_leaves::<Sha256>(&validators_bytes);

        // Convert the expected hashes to hex strings.
        let expected_digests: Vec<String> = expected_digests_bytes
            .iter()
            .map(|x| String::from_utf8(hex::encode(x)).expect("Invalid UTF-8"))
            .collect::<Vec<_>>();

        // Convert the expected hashes bytes to bits.
        let digests_bits: Vec<Vec<bool>> = expected_digests
            .iter()
            .map(|x| to_bits(hex::decode(x).unwrap()))
            .collect();

        let (validators_target, validator_byte_length, _) =
            generate_inputs(&mut builder, &validators);

        let result = builder.hash_validator_leaves(&validators_target, &validator_byte_length);
        println!("Got all leaf hashes: {}", result.len());
        for i in 0..validators.len() {
            for j in 0..HASH_SIZE_BITS {
                if digests_bits[i][j] {
                    pw.set_target(result[i][j].target, F::ONE);
                } else {
                    pw.set_target(result[i][j].target, F::ZERO);
                }
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_generate_val_hash_normal() {
        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        // Generated array with byte arrays with variable length [38, 46] bytes (to mimic validator bytes), and computed the validator hash corresponding to a merkle tree of depth 2 formed by these validator bytes.
        let validators: Vec<&str> = vec!["6694200ba0e084f7184255abedc39af04463a4ff11e0e0c1326b1b82ea1de50c6b35cf6efa8f7ed3", "739d312e54353379a852b43de497ca4ec52bb49f59b7294a4d6cf19dd648e16cb530b7a7a1e35875d4ab4d90", "4277f2f871f3e041bcd4643c0cf18e5a931c2bfe121ce8983329a289a2b0d2161745a2ddf99bade9a1"];

        let (validators_target, validator_byte_length, validator_enabled) =
            generate_inputs(&mut builder, &validators);

        let expected_digest = "d3430135bc6ed16a421ef1b8ec45d4d8b3e335e479f2bc3b074e9f1ed1d8f67e";
        let digest_bits = to_bits(hex::decode(expected_digest).unwrap());

        println!(
            "Expected Val Hash Encoding (Bytes): {:?}",
            hex::decode(expected_digest).unwrap()
        );

        let result = builder.hash_validator_set(
            &validators_target,
            &validator_byte_length,
            &validator_enabled,
        );

        for i in 0..HASH_SIZE_BITS {
            if digest_bits[i] {
                pw.set_target(result[i].target, F::ONE);
            } else {
                pw.set_target(result[i].target, F::ZERO);
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        println!("Created proof");

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_generate_val_hash_small() {
        // Generate the val hash for a small number of validators (would fit in a tree of less than max depth)
        let mut pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        // Generated array with byte arrays with variable length [38, 46] bytes (to mimic validator bytes), and computed the validator hash corresponding to a merkle tree of depth 2 formed by these validator bytes.
        let validators: Vec<&str> = vec!["364db94241a02b701d0dc85ac016fab2366fba326178e6f11d8294931969072b7441fd6b0ff5129d6867", "6fa0cef8f328eb8e2aef2084599662b1ee0595d842058966166029e96bd263e5367185f19af67b099645ec08aa"];

        let (validators_target, validator_byte_length, validator_enabled) =
            generate_inputs(&mut builder, &validators);

        let expected_digest = "be110ff9abb6bdeaebf48ac8e179a76fda1f6eaef0150ca6159587f489722204";
        let digest_bits = to_bits(hex::decode(expected_digest).unwrap());

        println!(
            "Expected Val Hash Encoding (Bytes): {:?}",
            hex::decode(expected_digest).unwrap()
        );

        let result = builder.hash_validator_set(
            &validators_target,
            &validator_byte_length,
            &validator_enabled,
        );

        for i in 0..HASH_SIZE_BITS {
            if digest_bits[i] {
                pw.set_target(result[i].target, F::ONE);
            } else {
                pw.set_target(result[i].target, F::ZERO);
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        println!("Created proof");

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_accumulate_voting_power() {
        let test_cases = [
            // voting power, enabled, pass
            (vec![10i64, 10i64, 10i64, 10i64], [1, 1, 1, 0], true),
            (vec![10i64, 10i64, 10i64, 10i64], [1, 1, 1, 1], true),
            (
                vec![4294967296000i64, 4294967296i64, 10i64, 10i64],
                [1, 0, 0, 0],
                true,
            ),
            (
                vec![4294967296000i64, 4294967296000i64, 4294967296000i64, 0i64],
                [1, 1, 0, 0],
                true,
            ),
        ];

        // These test cases should pass
        for test_case in test_cases {
            let mut pw = PartialWitness::new();
            let config = CircuitConfig::standard_recursion_config();
            let mut builder = CircuitBuilder::<F, D>::new(config);

            let mut all_validators = vec![];
            let mut validators_enabled = vec![];
            let mut total_vp = 0;
            for i in 0..test_case.0.len() {
                let voting_power = test_case.0[i];
                total_vp += voting_power;
                let voting_power_lower = voting_power & ((1 << 32) - 1);
                let voting_power_upper = voting_power >> 32;

                let voting_power_lower_target = U32Target(
                    builder.constant(F::from_canonical_usize(voting_power_lower as usize)),
                );
                let voting_power_upper_target = U32Target(
                    builder.constant(F::from_canonical_usize(voting_power_upper as usize)),
                );
                let voting_power_target =
                    I64Target([voting_power_lower_target, voting_power_upper_target]);

                all_validators.push(voting_power_target);
                validators_enabled.push(builder.constant_u32(test_case.1[i]));
            }

            let total_vp_lower = total_vp & ((1 << 32) - 1);
            let total_vp_upper = total_vp >> 32;

            println!("Lower total vp: {:?}", total_vp_lower);
            println!("Upper total vp: {:?}", total_vp_upper);

            let total_vp_lower_target =
                U32Target(builder.constant(F::from_canonical_usize(total_vp_lower as usize)));
            let total_vp_upper_target =
                U32Target(builder.constant(F::from_canonical_usize(total_vp_upper as usize)));
            let total_vp_target = I64Target([total_vp_lower_target, total_vp_upper_target]);

            let two_u32 = builder.constant_u32(2);
            let three_u32 = builder.constant_u32(3);

            let result = builder.check_voting_power(
                &all_validators,
                &validators_enabled,
                &total_vp_target,
                two_u32,
                three_u32,
            );

            pw.set_bool_target(result, test_case.2);

            let data = builder.build::<C>();
            let proof = data.prove(pw).unwrap();

            println!("Created proof");

            data.verify(proof).unwrap();

            println!("Verified proof");
        }
    }

    #[test]
    fn test_verify_hash_in_message() {
        // This is a test case generated from block 144094 of Celestia's Mocha testnet
        // Block Hash: 8909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c (needs to be lower case)
        // Signed Message (from the last validator): 6b080211de3202000000000022480a208909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c12240801122061263df4855e55fcab7aab0a53ee32cf4f29a1101b56de4a9d249d44e4cf96282a0b089dce84a60610ebb7a81932076d6f6368612d33
        // No round exists in present the message that was signed above

        let header_hash = "8909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c";
        let header_bits = to_bits(hex::decode(header_hash).unwrap());

        let signed_message = "6b080211de3202000000000022480a208909e1b73b7d987e95a7541d96ed484c17a4b0411e98ee4b7c890ad21302ff8c12240801122061263df4855e55fcab7aab0a53ee32cf4f29a1101b56de4a9d249d44e4cf96282a0b089dce84a60610ebb7a81932076d6f6368612d33";
        let signed_message_bits = to_bits(hex::decode(signed_message).unwrap());

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

        let result =
            builder.verify_hash_in_message(signed_message_target, header_hash_target, zero);

        for i in 0..HASH_SIZE_BITS {
            if header_bits[i] {
                pw.set_target(result[i].target, F::ONE);
            } else {
                pw.set_target(result[i].target, F::ZERO);
            }
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        println!("Created proof");

        data.verify(proof).unwrap();

        println!("Verified proof");
    }

    #[test]
    fn test_marshal_int64_varint() {
        // These are test cases generated from `celestia-core`.
        //
        // allZerosPubkey := make(ed25519.PubKey, ed25519.PubKeySize)
        // votingPower := int64(9999999999999)
        // validator := NewValidator(allZerosPubkey, votingPower)
        // fmt.Println(validator.Bytes()[37:])
        //
        // The tuples hold the form: (voting_power_i64, voting_power_varint_bytes).
        let test_cases = [
            (1i64, vec![1u8]),
            (1234567890i64, vec![210, 133, 216, 204, 4]),
            (38957235239i64, vec![167, 248, 160, 144, 145, 1]),
            (9999999999999i64, vec![255, 191, 202, 243, 132, 163, 2]),
            (
                724325643436111i64,
                vec![207, 128, 183, 165, 211, 216, 164, 1],
            ),
            (
                9223372036854775807i64,
                vec![255, 255, 255, 255, 255, 255, 255, 255, 127],
            ),
        ];

        for test_case in test_cases {
            let pw = PartialWitness::new();
            let config = CircuitConfig::standard_recursion_config();
            let mut builder = CircuitBuilder::<F, D>::new(config);

            // TODO: Need to add check in marshal that this is not negative
            let voting_power_i64 = test_case.0;
            let voting_power_lower = voting_power_i64 & ((1 << 32) - 1);
            let voting_power_upper = voting_power_i64 >> 32;

            let voting_power_lower_target =
                U32Target(builder.constant(F::from_canonical_usize(voting_power_lower as usize)));
            let voting_power_upper_target =
                U32Target(builder.constant(F::from_canonical_usize(voting_power_upper as usize)));
            let voting_power_target =
                I64Target([voting_power_lower_target, voting_power_upper_target]);
            let result = builder.marshal_int64_varint(voting_power_target);

            for i in 0..result.len() {
                builder.register_public_input(result[i].target);
            }

            let data = builder.build::<C>();
            let proof = data.prove(pw).unwrap();

            let marshalled_bytes = f_bits_to_bytes(&proof.public_inputs);
            let expected_bytes = test_case.1;

            println!("Voting Power: {:?}", test_case.0);
            println!("Expected Varint Encoding (Bytes): {:?}", expected_bytes);
            println!("Produced Varint Encoding (Bytes): {:?}", marshalled_bytes);

            for i in 0..marshalled_bytes.len() {
                if i >= expected_bytes.len() {
                    assert_eq!(marshalled_bytes[i], 0);
                    continue;
                }
                assert_eq!(marshalled_bytes[i], expected_bytes[i]);
            }
        }
    }

    #[test]
    fn test_marshal_tendermint_validator() {
        // This is a test cases generated from `celestia-core`.
        //
        // allZerosPubkey := make(ed25519.PubKey, ed25519.PubKeySize)
        // minimumVotingPower := int64(724325643436111)
        // minValidator := NewValidator(allZerosPubkey, minimumVotingPower)
        // fmt.Println(minValidator.Bytes())
        //
        // The tuples hold the form: (voting_power_i64, voting_power_varint_bytes).
        let voting_power_i64 = 724325643436111i64;
        let pubkey_bits = [false; 256];
        let expected_marshal = [
            10u8, 34, 10, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 16, 207, 128, 183, 165, 211, 216, 164, 1,
        ];

        let pw = PartialWitness::new();
        let config = CircuitConfig::standard_recursion_config();
        let mut builder = CircuitBuilder::<F, D>::new(config);

        let voting_power_lower = voting_power_i64 & ((1 << 32) - 1);
        let voting_power_upper = voting_power_i64 >> 32;

        let voting_power_lower_target =
            U32Target(builder.constant(F::from_canonical_usize(voting_power_lower as usize)));
        let voting_power_upper_target =
            U32Target(builder.constant(F::from_canonical_usize(voting_power_upper as usize)));
        let voting_power_target = I64Target([voting_power_lower_target, voting_power_upper_target]);

        let mut pubkey = [builder._false(); 256];
        for i in 0..256 {
            pubkey[i] = if pubkey_bits[i] {
                builder._true()
            } else {
                builder._false()
            };
        }
        let pubkey = Ed25519PubkeyTarget(pubkey);
        let result = builder.marshal_tendermint_validator(pubkey, voting_power_target);

        for i in 0..result.len() {
            builder.register_public_input(result[i].target);
        }

        let data = builder.build::<C>();
        let proof = data.prove(pw).unwrap();

        let marshalled_bytes = f_bits_to_bytes(&proof.public_inputs);
        let expected_bytes = expected_marshal;

        println!("Voting Power: {:?}", voting_power_i64);
        println!("Public Key: {:?}", bits_to_bytes(&pubkey_bits));
        println!("Expected Validator Encoding (Bytes): {:?}", expected_bytes);
        println!(
            "Produced Validator Encoding (Bytes): {:?}",
            marshalled_bytes
        );

        for i in 0..marshalled_bytes.len() {
            if i >= expected_bytes.len() {
                assert_eq!(marshalled_bytes[i], 0);
                continue;
            }
            assert_eq!(marshalled_bytes[i], expected_bytes[i]);
        }
    }
}
