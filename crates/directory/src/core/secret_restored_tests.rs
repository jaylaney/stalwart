/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

#[test]
fn is_password_hash_detects_phc_strings() {
    let argon = Argon2::default()
        .hash_password(b"hello")
        .unwrap()
        .to_string();
    assert!(is_password_hash(&argon), "argon2 not detected: {argon}");

    let pbkdf = Pbkdf2::default()
        .hash_password(b"hello")
        .unwrap()
        .to_string();
    assert!(is_password_hash(&pbkdf), "pbkdf2 not detected: {pbkdf}");

    let scr = Scrypt::default()
        .hash_password(b"hello")
        .unwrap()
        .to_string();
    assert!(is_password_hash(&scr), "scrypt not detected: {scr}");
}

#[test]
fn is_password_hash_detects_crypt_variants() {
    let bc = bcrypt::hash("hello").unwrap();
    assert!(is_password_hash(&bc), "bcrypt not detected: {bc}");
    assert!(bcrypt::verify("hello", &bc));

    let md5 = "$1$5pZSV9va$azfrPr6af3Fc7dLblQXVa0";
    assert!(is_password_hash(md5));
    assert!(md5_crypt::verify("password", md5));

    let sha256 = "$5$WH1ABM5sKhxbkgCK$sOnTVjQn1Y3EWibd8gWqqJqjH.KaFrxJE5rijqxcPp7";
    assert!(is_password_hash(sha256));
    assert!(sha256_crypt::verify("test", sha256));

    let sha256_rounds =
        "$5$rounds=11858$WH1ABM5sKhxbkgCK$aTQsjPkz0rBsH3lQlJxw9HDTDXPKBxC0LlVeV69P.t1";
    assert!(is_password_hash(sha256_rounds));
    assert!(sha256_crypt::verify("test", sha256_rounds));

    let s512 = sha512_crypt::hash("hello").unwrap();
    assert!(is_password_hash(&s512), "sha512_crypt not detected: {s512}");
    assert!(sha512_crypt::verify("hello", &s512));

    let s1 = sha1_crypt::hash("hello").unwrap();
    assert!(is_password_hash(&s1), "sha1_crypt not detected: {s1}");
    assert!(sha1_crypt::verify("hello", &s1));

    let bsdi = "_J9..K0AyUubDkQmPLeM";
    assert!(is_password_hash(bsdi), "bsdi_crypt not detected: {bsdi}");
}

#[test]
fn is_password_hash_detects_ldap_schemes() {
    let mut h = Sha1::new();
    h.update(b"hello");
    let sha = b64(&h.finalize()[..]);
    assert!(is_password_hash(&format!("{{SHA}}{sha}")));

    let mut h = Sha1::new();
    h.update(b"hello");
    h.update(b"saltbytes");
    let mut buf = h.finalize().to_vec();
    buf.extend_from_slice(b"saltbytes");
    let ssha = b64(&buf);
    assert!(is_password_hash(&format!("{{SSHA}}{ssha}")));

    let mut h = Sha256::new();
    h.update(b"hello");
    let sha256 = b64(&h.finalize()[..]);
    assert!(is_password_hash(&format!("{{SHA256}}{sha256}")));

    let mut h = Sha256::new();
    h.update(b"hello");
    h.update(b"saltbytes");
    let mut buf = h.finalize().to_vec();
    buf.extend_from_slice(b"saltbytes");
    let ssha256 = b64(&buf);
    assert!(is_password_hash(&format!("{{SSHA256}}{ssha256}")));

    let mut h = Sha512::new();
    h.update(b"hello");
    let sha512 = b64(&h.finalize()[..]);
    assert!(is_password_hash(&format!("{{SHA512}}{sha512}")));

    let mut h = Sha512::new();
    h.update(b"hello");
    h.update(b"saltbytes");
    let mut buf = h.finalize().to_vec();
    buf.extend_from_slice(b"saltbytes");
    let ssha512 = b64(&buf);
    assert!(is_password_hash(&format!("{{SSHA512}}{ssha512}")));

    let digest = md5::compute(b"hello");
    let md5b = b64(&digest[..]);
    assert!(is_password_hash(&format!("{{MD5}}{md5b}")));

    let inner = sha512_crypt::hash("hello").unwrap();
    assert!(is_password_hash(&format!("{{CRYPT}}{inner}")));
    assert!(is_password_hash(&format!("{{crypt}}{inner}")));

    assert!(is_password_hash(
        "{CRYPT}$1$5pZSV9va$azfrPr6af3Fc7dLblQXVa0"
    ));
    assert!(is_password_hash("{CRYPT}abcdefghij012"));
    assert!(is_password_hash("{CRYPT}_J9..K0AyUubDkQmPLeM"));

    let a = Argon2::default()
        .hash_password(b"hello")
        .unwrap()
        .to_string();
    assert!(is_password_hash(&format!("{{ARGON2ID}}{a}")));
    assert!(is_password_hash(&format!("{{ARGON2}}{a}")));
    assert!(is_password_hash(&format!("{{ARGON2I}}{a}")));

    let p = Pbkdf2::default()
        .hash_password(b"hello")
        .unwrap()
        .to_string();
    assert!(is_password_hash(&format!("{{PBKDF2}}{p}")));

    let mut h = Sha1::new();
    h.update(b"hello");
    let sha_lc = b64(&h.finalize()[..]);
    assert!(is_password_hash(&format!("{{sha}}{sha_lc}")));

    let mut h = Sha256::new();
    h.update(b"hello");
    h.update(b"saltbytes");
    let mut buf = h.finalize().to_vec();
    buf.extend_from_slice(b"saltbytes");
    let ssha256_lc = b64(&buf);
    assert!(is_password_hash(&format!("{{ssha256}}{ssha256_lc}")));

    let digest = md5::compute(b"hello");
    let md5_mc = b64(&digest[..]);
    assert!(is_password_hash(&format!("{{Md5}}{md5_mc}")));
}

#[test]
fn is_password_hash_rejects_passwords() {
    let not_hashes = [
        "",
        "hello",
        "p@ssw0rd!",
        "password123",
        "correct horse battery staple",
        "$myPassword",
        "$1incomplete",
        "$1$",
        "$1$short",
        "$1$abc$tooshorthash",
        "$5$",
        "$5$nohashpart$",
        "$5$saltonly$alsotooshort",
        "$6$",
        "$$$",
        "$$argon2$",
        "$argon2id$broken",
        "$argon2id$v=19$bad",
        "$2",
        "$2y$",
        "$2y$10$short",
        "$2z$10$N9qo8uLOickgx2ZMRZoMyeIjZAgcfl7p92ldGxad68LJZdL17lhWy",
        "$sha1$",
        "$sha1$notdigits$salt$hash",
        "{",
        "{}",
        "{}foo",
        "{SHA}",
        "{SHA}not!valid!base!64",
        "{SHA}aGVsbG8=",
        "{MD5}",
        "{MD5}aGVsbG8=",
        "{SHA256}aGVsbG8=",
        "{SHA512}aGVsbG8=",
        "{SSHA}aGVsbG8=",
        "{UNKNOWN}whatever",
        "{PLAIN}stillplain",
        "{plain}stillplain",
        "{CLEAR}stillplain",
        "{clear}stillplain",
        "{CRYPT}plainpw",
        "{CRYPT}",
        "{CRYPT}toolongtobeunixcryptbutshortbsdi",
        "{ARGON2ID}notaphcstring",
        "_short",
        "_notvalidbsdi",
        "regular_password",
        "1234567890123",
        "abcdefghij012",
        "$5$rounds=$saltvalue$abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJK",
    ];
    for p in not_hashes {
        assert!(!is_password_hash(p), "false positive: {p:?}");
    }
}
