use crate::merkle::{SignedBlock, TempSignedBlock};
use rand::Rng;
use reqwest::Error;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{fs::File, io::Write};
use subtle_encoding::hex;
use tendermint::{merkle::simple_hash_from_byte_vectors, validator::Set as ValidatorSet};

#[derive(Debug, Deserialize)]
struct Response {
    _jsonrpc: String,
    _id: i32,
    result: TempSignedBlock,
}

#[derive(Debug, Serialize, Deserialize)]
struct VerifySignatureData {
    pubkey: String,
    signature: String,
    message: String,
}

pub fn generate_val_array(num_validators: usize) {
    let mut rng = rand::thread_rng();
    // Generate an array of byte arrays where the byte arrays have variable length between 38 and 46 bytes and the total length of the array is less than n
    let random_bytes: Vec<Vec<u8>> = (0..num_validators)
        .map(|_| {
            let inner_length = rng.gen_range(38..=46);
            (0..inner_length).map(|_| rng.gen()).collect()
        })
        .collect();

    // Use simple_hash_from_byte_vectors to generate the root hash
    let root_hash = simple_hash_from_byte_vectors::<Sha256>(&random_bytes);

    // Print the random byte arrays as an array of hex strings, that have double quotes around them and are separated by commas

    let mut hex_strings = Vec::new();

    for b in &random_bytes {
        let hex_string = String::from_utf8(hex::encode(b)).expect("Found invalid UTF-8");
        hex_strings.push(hex_string);
    }

    // Format the hex strings with double quotes and commas
    println!("Validators: {:?}", hex_strings);

    // Print the root hash
    println!(
        "Root Hash: {:?}",
        String::from_utf8(hex::encode(root_hash)).expect("Found invalid UTF-8")
    );
}

pub async fn get_celestia_consensus_signatures() -> Result<(), Error> {
    // Read from https://rpc-t.celestia.nodestake.top/signed_block?height=131950 using
    // Serves latest block
    let height = 11000;
    let mut url =
        "http://rpc.testnet.celestia.citizencosmos.space/signed_block?height=".to_string();
    url.push_str(height.to_string().as_str());

    // Send a GET request and wait for the response

    // Convert response to string
    let res = reqwest::get(url).await?.text().await?;

    let v: Response = serde_json::from_str(&res).expect("Failed to parse JSON");

    let temp_block = v.result;

    // Cast to SignedBlock
    let block = SignedBlock {
        header: temp_block.header,
        data: temp_block.data,
        commit: temp_block.commit,
        validator_set: ValidatorSet::new(
            temp_block.validator_set.validators,
            temp_block.validator_set.proposer,
        ),
    };

    println!("here");

    // Write to JSON file
    let json = serde_json::to_string(&block).unwrap();

    let mut path = "src/fixtures/".to_string();
    path.push_str(height.to_string().as_str());
    path.push_str("/signed_block.json");
    println!("Path: {:?}", path);
    let mut file = File::create(path).unwrap();
    file.write_all(json.as_bytes()).unwrap();

    Ok(())
}
