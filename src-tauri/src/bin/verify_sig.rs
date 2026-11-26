use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use std::fs;
use std::path::PathBuf;

/// Verifies Ed25519/minisign signatures against the exact public key embedded in `src-tauri/tauri.conf.json`:
/// 1. Valid signed artifact MUST pass.
/// 2. Corrupted artifact (1-byte flip) MUST fail.
/// 3. Tampered signature MUST fail.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: verify_updater_sig <tauri.conf.json> <artifact.exe> <artifact.exe.sig>");
        std::process::exit(2);
    }

    let conf_path = PathBuf::from(&args[1]);
    let artifact_path = PathBuf::from(&args[2]);
    let sig_path = PathBuf::from(&args[3]);

    let conf_raw = fs::read_to_string(&conf_path).expect("Failed to read tauri.conf.json");
    let conf_json: serde_json::Value =
        serde_json::from_str(&conf_raw).expect("Invalid tauri.conf.json");
    let pubkey_b64 = conf_json["plugins"]["updater"]["pubkey"]
        .as_str()
        .expect("Missing plugins.updater.pubkey")
        .trim();

    let pubkey_decoded = base64::engine::general_purpose::STANDARD
        .decode(pubkey_b64)
        .expect("Invalid base64 pubkey");
    let pubkey_str = String::from_utf8(pubkey_decoded).expect("Pubkey not UTF-8");
    let pk = PublicKey::decode(&pubkey_str).expect("Failed to decode minisign PublicKey");

    let sig_b64 = fs::read_to_string(&sig_path)
        .expect("Failed to read .sig")
        .trim()
        .to_string();
    let sig_decoded = base64::engine::general_purpose::STANDARD
        .decode(&sig_b64)
        .expect("Invalid base64 signature");
    let sig_str = String::from_utf8(sig_decoded).expect("Signature not UTF-8");
    let sig = Signature::decode(&sig_str).expect("Failed to decode minisign Signature");

    let artifact_bytes = fs::read(&artifact_path).expect("Failed to read artifact");

    // 1. Valid artifact + valid signature -> MUST SUCCEED
    pk.verify(&artifact_bytes, &sig, false)
        .expect("FAIL: valid artifact did NOT pass Ed25519 verification");
    println!(
        "PASS [1/3]: Valid Ed25519 signature verified for {} ({} bytes)",
        artifact_path.display(),
        artifact_bytes.len()
    );

    // 2. Corrupted artifact (flip 1 byte in middle) -> MUST FAIL
    let mut corrupted = artifact_bytes.clone();
    let mid = corrupted.len() / 2;
    corrupted[mid] ^= 0xFF;
    match pk.verify(&corrupted, &sig, false) {
        Ok(()) => {
            eprintln!("FAIL [2/3]: Corrupted artifact unexpectedly passed verification!");
            std::process::exit(1);
        }
        Err(err) => {
            println!("PASS [2/3]: Corrupted installer (1-byte flip) safely REJECTED ({err})");
        }
    }

    // 3. Tampered signature (flip last character of signature payload) -> MUST FAIL
    let mut sig_lines: Vec<String> = sig_str.lines().map(String::from).collect();
    if sig_lines.len() >= 2 {
        let mut chars: Vec<char> = sig_lines[1].chars().collect();
        if let Some(last) = chars.last_mut() {
            *last = if *last == 'A' { 'B' } else { 'A' };
        }
        sig_lines[1] = chars.into_iter().collect();
    }
    let tampered_sig_str = sig_lines.join("\n");
    match Signature::decode(&tampered_sig_str).and_then(|bad_sig| pk.verify(&artifact_bytes, &bad_sig, false)) {
        Ok(()) => {
            eprintln!("FAIL [3/3]: Tampered signature unexpectedly passed verification!");
            std::process::exit(1);
        }
        Err(err) => {
            println!("PASS [3/3]: Invalid/tampered signature safely REJECTED ({err})");
        }
    }

    println!("ALL CRYPTOGRAPHIC UPDATER SIGNATURE CHECKS PASSED!");
}
