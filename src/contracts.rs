use ethers::prelude::abigen;

abigen!(
    ZKRollupBridge,
    r#"[
  {
    "inputs": [
      {
        "internalType": "bytes",
        "name": "batchData",
        "type": "bytes"
      },
      {
        "internalType": "bytes32",
        "name": "newRoot",
        "type": "bytes32"
      },
      {
        "components": [
          {
            "internalType": "uint256[2]",
            "name": "a",
            "type": "uint256[2]"
          },
          {
            "internalType": "uint256[2][2]",
            "name": "b",
            "type": "uint256[2][2]"
          },
          {
            "internalType": "uint256[2]",
            "name": "c",
            "type": "uint256[2]"
          }
        ],
        "internalType": "struct Groth16Proof",
        "name": "proof",
        "type": "tuple"
      }
    ],
    "name": "commitBatchCalldata",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
    "inputs": [
      {
        "internalType": "bytes32",
        "name": "expectedVersionedHash",
        "type": "bytes32"
      },
      {
        "internalType": "uint8",
        "name": "blobIndex",
        "type": "uint8"
      },
      {
        "internalType": "bool",
        "name": "useOpcodeBlobhash",
        "type": "bool"
      },
      {
        "internalType": "bytes32",
        "name": "newRoot",
        "type": "bytes32"
      },
      {
        "components": [
          {
            "internalType": "uint256[2]",
            "name": "a",
            "type": "uint256[2]"
          },
          {
            "internalType": "uint256[2][2]",
            "name": "b",
            "type": "uint256[2][2]"
          },
          {
            "internalType": "uint256[2]",
            "name": "c",
            "type": "uint256[2]"
          }
        ],
        "internalType": "struct Groth16Proof",
        "name": "proof",
        "type": "tuple"
      }
    ],
    "name": "commitBatchBlob",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
  }
]"#,
);
