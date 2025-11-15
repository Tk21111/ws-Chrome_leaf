// This file generates a self-signed cert for testing.
// Run it with: cargo test --test gen_cert -- --nocapture
use std::io::Write;

#[test]
fn gen_cert() {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();

    // Save the files
    let mut cert_file = std::fs::File::create("cert.der").unwrap();
    cert_file.write_all(&cert_der).unwrap();

    let mut key_file = std::fs::File::create("key.der").unwrap();
    key_file.write_all(&priv_key).unwrap();

    println!("Generated cert.der and key.der");
}