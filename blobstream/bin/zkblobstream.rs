use std::env;
use std::sync::Arc;

use ethers::contract::abigen;
use ethers::core::types::Address;
use ethers::prelude::SignerMiddleware;
use ethers::providers::{Http, Provider};
use ethers::signers::{LocalWallet, Signer, Wallet};
use ethers::types::H256;
use log::info;
use subtle_encoding::hex;
use tendermint::block::Header;
use zk_tendermint::input::tendermint_utils::HeaderResponse;

// Note: Update ABI when updating contract.
abigen!(ZKBlobstream, "./abi/ZKBlobstream.abi.json");

async fn get_latest_header(base_url: String) -> Header {
    let query_url = format!("{}/header", base_url);
    info!("Querying url {:?}", query_url.as_str());
    let res = reqwest::get(query_url).await.unwrap().text().await.unwrap();
    let v: HeaderResponse = serde_json::from_str(&res).expect("Failed to parse JSON");
    v.result.header
}

async fn get_header_from_number(base_url: String, block_number: u64) -> Header {
    let query_url = format!("{}/header?height={}", base_url, block_number);
    info!("Querying url {:?}", query_url.as_str());
    let res = reqwest::get(query_url).await.unwrap().text().await.unwrap();
    let v: HeaderResponse = serde_json::from_str(&res).expect("Failed to parse JSON");
    v.result.header
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    dotenv::dotenv().ok();

    let tendermint_rpc_url = env::var("RPC_MOCHA_4").expect("RPC_MOCHA_4 must be set");

    let ethereum_rpc_url = env::var("RPC_URL").expect("RPC_URL must be set");
    let provider =
        Provider::<Http>::try_from(ethereum_rpc_url).expect("could not connect to client");

    let private_key = env::var("PRIVATE_KEY").expect("PRIVATE_KEY must be set");
    let wallet: LocalWallet = private_key
        .parse::<LocalWallet>()
        .expect("invalid private key")
        .with_chain_id(5u64);

    info!("Wallet address: {:?}", wallet.address());

    let client = SignerMiddleware::new(provider, wallet);
    let client = Arc::new(client);

    // ZKBlobstream on Goerli: https://goerli.etherscan.io/address/0x67ea962864cdad3f2202118dc6f65ff510f7bb4d#code
    let address = "0x67ea962864cdad3f2202118dc6f65ff510f7bb4d";
    let address = address.parse::<Address>().expect("invalid address");

    let zk_blobstream = ZKBlobstream::new(address, client.clone());
    let latest_header = get_latest_header(tendermint_rpc_url.clone()).await;
    let latest_block = latest_header.height.value();

    println!("Latest block: {}", latest_block);

    // let genesis_header =
    //     get_header_from_number(tendermint_rpc_url.clone(), latest_block - 500).await;

    // zk_blobstream
    //     .set_genesis_header(
    //         latest_block - 500,
    //         H256::from_slice(genesis_header.hash().as_bytes()).0,
    //     )
    //     .send()
    //     .await
    //     .expect("failed to set genesis header");

    // let mut curr_block = 10300;

    let mut calls_so_far = 0;

    // Loop every 20 minutes. Call request_combined_skip every 30 minutes with the latest block number.
    loop {
        // Verify the call succeeded.

        let latest_header = get_latest_header(tendermint_rpc_url.clone()).await;
        let latest_block = latest_header.height.value();

        let block_to_request = latest_block - 10;

        println!("Requesting combined skip for block {}", block_to_request);

        zk_blobstream
            .request_combined_skip(block_to_request)
            .send()
            .await
            .expect("failed to request combined skip");

        tokio::time::sleep(tokio::time::Duration::from_secs(60 * 30)).await;

        calls_so_far += 1;
        if calls_so_far == 20 {
            break;
        }
        // curr_block += 100;
    }

    Ok(())
}
