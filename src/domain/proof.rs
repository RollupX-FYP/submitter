use tracing::{info, warn};

/// Formats a proof payload according to the expected requirements of the L1 Verifier.
/// Groth16 (verifier_id == 0) strictly requires exactly 256 bytes.
pub fn format_proof_for_verifier(mut proof_bytes: Vec<u8>, verifier_id: u8) -> Vec<u8> {
    if verifier_id == 0 {
        // Groth16 requires exactly 256 bytes
        if proof_bytes.len() < 256 {
            info!("Padding Groth16 proof from {} bytes to 256 bytes", proof_bytes.len());
            proof_bytes.resize(256, 0);
        } else if proof_bytes.len() > 256 {
            warn!("Groth16 proof length {} > 256, truncating", proof_bytes.len());
            proof_bytes.truncate(256);
        }
    } else {
        // Plonky2/Halo2: Pass raw or padded, just log it
        info!("Non-Groth16 proof length: {} bytes", proof_bytes.len());
    }
    proof_bytes
}
