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
    "name": "commitBatch",
    "outputs": [],
    "stateMutability": "nonpayable",
    "type": "function"
  },
  {
      "inputs": [],
      "name": "stateRoot",
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

pub fn parse_groth16_proof(hex_proof: &str) -> Result<Groth16Proof, String> {
    let hex_proof = hex_proof.trim_start_matches("0x");
    let bytes = ethers::utils::hex::decode(hex_proof).map_err(|e| format!("Invalid hex: {}", e))?;

    if bytes.len() != 256 {
        return Err(format!("Invalid proof length: expected 256 bytes, got {}", bytes.len()));
    }

    let mut a = [ethers::types::U256::zero(); 2];
    let mut b = [[ethers::types::U256::zero(); 2]; 2];
    let mut c = [ethers::types::U256::zero(); 2];

    for i in 0..2 {
        a[i] = ethers::types::U256::from_big_endian(&bytes[i * 32..(i + 1) * 32]);
    }

    for i in 0..2 {
        for j in 0..2 {
            // Offset: 64 + (i*2 + j) * 32
            // i=0, j=0 -> 64
            // i=0, j=1 -> 96
            // i=1, j=0 -> 128
            // i=1, j=1 -> 160
            let start = 64 + (i * 2 + j) * 32;
            b[i][j] = ethers::types::U256::from_big_endian(&bytes[start..start + 32]);
        }
    }

    for i in 0..2 {
        let start = 192 + i * 32;
        c[i] = ethers::types::U256::from_big_endian(&bytes[start..start + 32]);
    }

    Ok(Groth16Proof { a, b, c })
}
