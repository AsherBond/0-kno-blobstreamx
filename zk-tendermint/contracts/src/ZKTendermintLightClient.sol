// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {IFunctionGateway} from "@succinctx/interfaces/IFunctionGateway.sol";
import {IZKTendermintLightClient} from "./IZKTendermintLightClient.sol";

contract ZKTendermintLightClient is IZKTendermintLightClient {
    event HeaderSkipRequested(
        uint64 indexed trustedBlock,
        uint64 indexed requestedBlock,
        bytes32 requestId
    );
    event HeaderSkipFulfilled(uint64 indexed requestedBlock, bytes32 header);

    event HeaderStepRequested(uint64 indexed prevBlock, bytes32 requestId);
    event HeaderStepFulfilled(uint64 indexed nextBlock, bytes32 header);

    event FunctionId(string name, bytes32 id);
    address public gateway;
    mapping(string => bytes32) public functionNameToId;

    mapping(uint64 => bytes32) public blockHeightToHeaderHash;
    uint64 latestBlock;

    modifier onlyGateway() {
        require(msg.sender == gateway, "Only gateway can call this function");
        _;
    }

    constructor(address _gateway) {
        gateway = _gateway;
    }

    function getLatestBlock() external view returns (uint64) {
        return latestBlock;
    }

    function getFunctionId(string memory name) external view returns (bytes32) {
        return functionNameToId[name];
    }

    function getHeaderHash(uint64 height) external view returns (bytes32) {
        return blockHeightToHeaderHash[height];
    }

    function updateGateway(address _gateway) external {
        gateway = _gateway;
    }

    function updateFunctionId(
        string memory name,
        bytes32 _functionId
    ) external {
        functionNameToId[name] = _functionId;
    }

    function setGenesisHeader(uint64 height, bytes32 header) external {
        blockHeightToHeaderHash[height] = header;
        latestBlock = height;
    }

    function requestHeaderSkip(uint64 _requestedBlock) external payable {
        bytes32 trustedHeader = blockHeightToHeaderHash[latestBlock];
        if (trustedHeader == bytes32(0)) {
            revert("Trusted header not found");
        }
        bytes32 id = functionNameToId["skip"];
        if (id == bytes32(0)) {
            revert("Function ID for skip not found");
        }
        require(_requestedBlock - latestBlock <= 512); // TODO: change this constant (should match max number of blocks in a data commitment)
        require(_requestedBlock > latestBlock); // TODO: do we need this?
        bytes32 requestId = IFunctionGateway(gateway).request{value: msg.value}(
            id,
            abi.encodePacked(trustedHeader, latestBlock, _requestedBlock),
            this.callbackHeaderSkip.selector,
            abi.encode(_requestedBlock)
        );
        emit HeaderSkipRequested(latestBlock, _requestedBlock, requestId);
    }

    function callbackHeaderSkip(
        bytes memory requestResult,
        bytes memory context
    ) external onlyGateway {
        uint64 requestedBlock = abi.decode(context, (uint64));
        bytes32 newHeader = abi.decode(requestResult, (bytes32));
        blockHeightToHeaderHash[requestedBlock] = newHeader;
        require(requestedBlock > latestBlock);
        latestBlock = requestedBlock;
        emit HeaderSkipFulfilled(requestedBlock, newHeader);
    }

    // Needed in the rare case that skip cannot be used--when validator set changes by > 1/3
    function requestHeaderStep() external payable {
        bytes32 headHeaderHash = blockHeightToHeaderHash[latestBlock];
        if (headHeaderHash == bytes32(0)) {
            revert("Prev header not found");
        }
        bytes32 id = functionNameToId["step"];
        if (id == bytes32(0)) {
            revert("Function ID for step not found");
        }
        emit FunctionId("step", id);
        bytes32 requestId = IFunctionGateway(gateway).request{value: msg.value}(
            id,
            abi.encodePacked(headHeaderHash, latestBlock),
            this.callbackHeaderStep.selector,
            abi.encode(latestBlock)
        );
        emit HeaderStepRequested(latestBlock, requestId);
    }

    function callbackHeaderStep(
        bytes memory requestResult,
        bytes memory context
    ) external onlyGateway {
        uint64 prevBlock = abi.decode(context, (uint64));
        bytes32 nextHeader = abi.decode(requestResult, (bytes32));
        uint64 nextBlock = prevBlock + 1;
        blockHeightToHeaderHash[nextBlock] = nextHeader;
        require(nextBlock > latestBlock);
        latestBlock = nextBlock;
        emit HeaderStepFulfilled(nextBlock, nextHeader);
    }
}