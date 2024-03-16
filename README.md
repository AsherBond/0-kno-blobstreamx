# Blobstream X

![Blobstream X](https://pbs.twimg.com/media/F85boT-bYAAF1hM?format=jpg&name=4096x4096)

Implementation of zero-knowledge proof circuits for [Blobstream](https://docs.celestia.org/developers/blobstream), Celestia's data availability solution for Ethereum.

## Overview

Blobstream X's core contract is `BlobstreamX`, which stores commitments to ranges of data roots from Celestia blocks. Users can query for the validity of a data root of a specific block height via `verifyAttestation`, which proves that the data root is a leaf in the Merkle tree for the block range the specific block height is in.

## Request BlobstreamX Proofs

### Request Proofs from the Succinct Platform

Add env variables to `.env`, following the `.env.example`. You do not need to fill out the local configuration, unless you're planning on doing local proving.

Run `BlobstreamX` script to request updates to the specified light client continuously. For the cadence of requesting updates, update `LOOP_DELAY_MINUTES`.

In `/`, run

```

cargo run --bin blobstreamx --release

```

### Generate & Relay Proofs Locally

To enable local proving & local relaying of proofs with the Blobstream X operator, download the proving binaries by following the instructions [here](https://hackmd.io/Q6CsiGOjTrCjD7UCAgiDBA#Download-artifacts).

Then, simply add the following to your `.env`:

```
LOCAL_PROVE_MODE=true
LOCAL_RELAY_MODE=true

# Add the path to each binary (ex. PROVE_BINARY_0x6d...=blobstream-artifacts/header_range)

PROVE_BINARY_0xFILL_IN_NEXT_HEADER_FUNCTION_ID=
PROVE_BINARY_0xFILL_IN_HEADER_RANGE_FUNCTION_ID=
WRAPPER_BINARY=
```

#### Relay an Existing Proof

Add env variables to `.env`, following the `.env.example`.

If you want to relay an existing proof in `/proofs`, run the following command:

```shell
cargo run --bin local_relay --release -- --request-id {REQUEST_ID}
```

## BlobstreamX Contract Overview

### Contract Deployment

To deploy the `BlobstreamX` contract:

1. Get the genesis parameters for a `BlobstreamX` contract from a specific Celestia block.

   ```shell
   cargo run --bin genesis -- --block <genesis_block>
   ```

2. Add .env variables to `contracts/.env`, following `contracts/.env.example`.
3. Initialize `BlobstreamX` contract with genesis parameters. In `contracts`, run

   ```shell
   forge install

   source .env

   forge script script/Deploy.s.sol --rpc-url $RPC_URL --private-key $PRIVATE_KEY --broadcast --verify --verifier etherscan --etherscan-api-key $ETHERSCAN_API_KEY
   ```

### Succinct Gateway Prover Whitelist

#### Set Whitelist Status

Set the whitelist status of a functionID to Default (0), Custom (1) or Disabled (2).

```shell
cast calldata "setWhitelistStatus(bytes32,uint8)" <YOUR_FUNCTION_ID> <WHITELIST_STATUS>
```

#### Add Custom Prover

Add a custom prover for a specific functionID.

```shell
cast calldata "addCustomProver(bytes32,address)" <FUNCTION_ID> <CUSTOM_PROVER_ADDRESS>
```
