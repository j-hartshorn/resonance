use anyhow::Result;
use qrcode::{render::unicode, QrCode};

/// Generates a text-based QR code for terminal display
pub fn generate_qr_code(data: &str) -> Result<String> {
    // Create QR code
    let code = QrCode::new(data)?;

    // Render as text using unicode characters
    let qr_string = code.render::<unicode::Dense1x2>().build();

    Ok(qr_string)
}

/// Displays connection sharing options in the terminal
pub fn display_connection_options(connection_link: &str) -> Result<()> {
    println!("===== Session Created =====");
    println!("Share this information with others to join your session:\n");

    // Option 1: Direct link
    println!("Connection Link:");
    println!("{}\n", connection_link);

    // Option 2: QR Code (terminal-based)
    println!("QR Code (scan with phone camera):");
    let qr = generate_qr_code(connection_link)?;
    println!("{}\n", qr);

    // Option 3: Command to join
    println!("Command Line:");
    println!("resonance join '{}'\n", connection_link);

    println!("Waiting for participants to join...");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_code_generation() {
        let data = "resonance://join?test=123";
        let qr = generate_qr_code(data).unwrap();

        // QR code should be non-empty
        assert!(!qr.is_empty());

        // QR code should contain multiple lines
        assert!(qr.contains('\n'));
    }
}
