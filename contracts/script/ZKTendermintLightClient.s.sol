// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import "forge-std/Script.sol";
import {ZKTendermintLightClient} from "../src/ZKTendermintLightClient.sol";

// forge script script/ZKTendermintLightClient.s.sol --verifier etherscan --private-key
// forge verify-contract <address> ZKTendermintLightClient --chain 5 --etherscan-api-key ${ETHERSCAN_API_KEY} --constructor-args "0x000000000000000000000000852a94f8309d445d27222edb1e92a4e83dddd2a8"
contract DeployScript is Script {
    function setUp() public {}

    function run() public {
        vm.startBroadcast();
        address gateway = address(0x852a94F8309D445D27222eDb1E92A4E83DdDd2a8);
        bytes32 functionId = bytes32(
            0x3f56e1405a9adc8c0f2a95d7723e42ea9afe05e2a30ac588289d8e26183a08bf
        );

        // Use the below to interact with an already deployed ZK light client
        ZKTendermintLightClient lightClient = ZKTendermintLightClient(
            0xB1cdc97E3C9fC29a30da31e49B4e2304b011d631
        );

        bytes32 header = hex"A8512F18C34B70E1533CFD5AA04F251FCB0D7BE56EC570051FBAD9BDB9435E6A";
        uint64 height = 3000;
        lightClient.setGenesisHeader(height, header);

        lightClient.updateFunctionId("step", functionId);

        lightClient.requestHeaderStep{value: 0.1 ether}(height);
    }
}
