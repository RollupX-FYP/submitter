#![cfg(not(tarpaulin_include))]

use ethers::prelude::abigen;

abigen!(
    ZKRollupBridge,
    r#"[
  {
    "inputs": [
      {
        "internalType": "uint8",
        "name": "daId",
        "type": "uint8"
      },
      {
        "internalType": "uint8",
        "name": "verifierId",
        "type": "uint8"
      },
      {
        "internalType": "bytes",
        "name": "batchData",
        "type": "bytes"
      },
      {
        "internalType": "bytes",
        "name": "daMeta",
        "type": "bytes"
      },
      {
        "internalType": "bytes32",
        "name": "newRoot",
        "type": "bytes32"
      },
      {
        "internalType": "bytes",
        "name": "proof",
        "type": "bytes"
      }
    ],
    "name": "commitBatch",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
      "inputs": [],
      "name": "latestStateRoot",
      "outputs": [
        {
          "internalType": "bytes32",
          "name": "",
          "type": "bytes32"
        }
      ],
      "stateMutability": "view",
      "type": "function"
  }
]"#,
);
