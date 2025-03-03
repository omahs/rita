//SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.21; // Force solidity compliance
import "@openzeppelin/contracts/token/ERC20/ERC20.sol";

// One of three testing coins named "Byecoin" with symbol "BYE", the constructor mints tokens
// and sends them to the input addresses, see hardhat.config.ts for the private keys
contract TestERC20C is ERC20 {
	constructor() ERC20("Byecoin", "BYE") {
		_mint(0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266, 100000000000000000000000000);
		_mint(0xc783df8a850f42e7F7e57013759C285caa701eB6, 10000);
		_mint(0xeAD9C93b79Ae7C1591b1FB5323BD777E86e150d4, 10000);
		_mint(0xE5904695748fe4A84b40b3fc79De2277660BD1D3, 10000);
		_mint(0x92561F28Ec438Ee9831D00D1D59fbDC981b762b2, 10000);
		_mint(0x2fFd013AaA7B5a7DA93336C2251075202b33FB2B, 10000);
		// this is the EtherBase address for our testnet miner in
		// tests/assets/ETHGenesis.json so it wil have both a lot
		// of ETH and a lot of erc20 tokens to test with
		_mint(0xBf660843528035a5A4921534E156a27e64B231fE, 100000000000000000000000000);
	}
}
