/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

// The 0.16 module this replaces held only its `tests` child, kept here
// under the same path.
mod tests {
    // 0.16's `utils::codec::base32_custom` moved to the external `encodify`
    // crate (`encodify::base32::STALWART`), which blob ids, state tokens and the
    // file-system, S3 and Azure blob paths now use. The round trip runs against
    // it, through the encoder and the streaming decoder those callers use.
    use encodify::base32::STALWART;

    #[test]
    fn base32_roundtrip() {
        let mut bytes = Vec::with_capacity(100);
        for byte in 0..100 {
            bytes.push((100 - byte) as u8);
            let result = STALWART.encode(&bytes);

            let mut bytes_result = Vec::new();
            for byte in STALWART.decoder(result.as_bytes()) {
                bytes_result.push(byte);
            }

            assert_eq!(bytes, bytes_result);
        }

        for bytes in [
            vec![0],
            vec![32, 43, 55, 99, 43, 55],
            vec![84, 4, 43, 77, 62, 55, 92],
            vec![84, 4, 43, 77, 62, 55, 92],
        ] {
            let result = STALWART.encode(&bytes);

            let mut bytes_result = Vec::new();
            for byte in STALWART.decoder(result.as_bytes()) {
                bytes_result.push(byte);
            }

            assert_eq!(bytes, bytes_result);
        }
    }
}
