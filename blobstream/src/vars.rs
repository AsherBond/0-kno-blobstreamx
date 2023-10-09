use plonky2x::frontend::merkle::tree::MerkleInclusionProofVariable;
use plonky2x::frontend::uint::uint64::U64Variable;
use plonky2x::prelude::{
    ArrayVariable, Bytes32Variable, CircuitBuilder, CircuitVariable, PlonkParameters, RichField,
    Variable,
};
use zk_tendermint::consts::*;

// The data commitment inputs as a struct.
// Note: data_hashes, data_hash_proofs should include range (start, end-1).
// Note: prev_header_proofs should include range (start+1, end).
#[derive(Clone, Debug, CircuitVariable)]
#[value_name(DataCommitmentProofValueType)]
pub struct DataCommitmentProofVariable<const MAX_LEAVES: usize> {
    pub data_hashes: ArrayVariable<Bytes32Variable, MAX_LEAVES>,
    pub end_header: Bytes32Variable,
    pub end_block_height: U64Variable,
    pub start_header: Bytes32Variable,
    pub start_block_height: U64Variable,
    pub data_hash_proofs: ArrayVariable<
        MerkleInclusionProofVariable<HEADER_PROOF_DEPTH, PROTOBUF_HASH_SIZE_BYTES>,
        MAX_LEAVES,
    >,
    pub prev_header_proofs: ArrayVariable<
        MerkleInclusionProofVariable<HEADER_PROOF_DEPTH, PROTOBUF_BLOCK_ID_SIZE_BYTES>,
        MAX_LEAVES,
    >,
}
