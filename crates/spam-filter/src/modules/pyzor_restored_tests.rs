/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::time::Duration;

use mail_parser::MessageParser;
use sha1::Digest;
use sha1::Sha1;
use utils::HexEncode;

use super::pyzor_create_message;
use super::pyzor_send_message;
use super::{PyzorDigest, html_to_text, pyzor_digest};

use super::PyzorResponse;

#[ignore]
#[tokio::test]
async fn send_message() {
    assert_eq!(
        pyzor_send_message(
            "public.pyzor.org:24441".parse().unwrap(),
            Duration::from_secs(10),
            concat!(
                "Op: check\n",
                "Op-Digest: b2c27325a034c581df0c9ef37e4a0d63208a3e7e\n",
                "Thread: 49005\n",
                "PV: 2.1\n",
                "User: anonymous\n",
                "Time: 1697468672\n",
                "Sig: 9cf4571b85d3887fdd0d4f444fd0c164e0290722\n"
            ),
        )
        .await
        .unwrap(),
        PyzorResponse {
            code: 200,
            count: 0,
            wl_count: 0
        }
    );
}

#[test]
fn message_pyzor() {
    let message = pyzor_create_message(
        &MessageParser::new().parse(HTML_TEXT_STYLE_SCRIPT).unwrap(),
        1697468672,
        49005,
    )
    .unwrap();

    assert_eq!(
        message,
        concat!(
            "Op: check\n",
            "Op-Digest: b2c27325a034c581df0c9ef37e4a0d63208a3e7e\n",
            "Thread: 49005\n",
            "PV: 2.1\n",
            "User: anonymous\n",
            "Time: 1697468672\n",
            "Sig: 9cf4571b85d3887fdd0d4f444fd0c164e0290722\n"
        )
    );
}

#[test]
fn digest_pyzor() {
    // HTML stripping
    assert_eq!(html_to_text(HTML_RAW), HTML_RAW_STRIPED);

    // Token stripping
    for strip_me in [
        "t@abc.com",
        "t1@abc.com",
        "t+a@abc.com",
        "t.a@abc.com",
        "0A2D3f%a#S",
        "3sddkf9jdkd9",
        "@@#@@@@@@@@@",
        "http://spammer.com/special-offers?buy=now",
    ] {
        assert_eq!(
            String::from_utf8(
                pyzor_digest(Vec::new(), format!("Test {strip_me} Test2").lines()).unwrap()
            )
            .unwrap(),
            "TestTest2"
        );
    }

    // Test short lines
    assert_eq!(
        String::from_utf8(
            pyzor_digest(
                Vec::new(),
                concat!("This line is included\n", "not this\n", "This also").lines(),
            )
            .unwrap()
        )
        .unwrap(),
        "ThislineisincludedThisalso"
    );

    // Test atomic
    assert_eq!(
        String::from_utf8(
            pyzor_digest(
                Vec::new(),
                "All this message\nShould be included\nIn the digest".lines(),
            )
            .unwrap()
        )
        .unwrap(),
        "AllthismessageShouldbeincludedInthedigest"
    );

    // Test spec
    let mut text = String::new();
    for i in 0..100 {
        text += format!("Line{i} test test test\n").as_str();
    }
    let mut expected = String::new();
    for i in [20, 21, 22, 60, 61, 62] {
        expected += format!("Line{i}testtesttest").as_str();
    }
    assert_eq!(
        String::from_utf8(pyzor_digest(Vec::new(), text.lines()).unwrap()).unwrap(),
        expected
    );

    // Test email parsing
    for (input, expected) in [
        (
            HTML_TEXT,
            concat!(
                "Emailspam,alsoknownasjunkemailorbulkemail,isasubset",
                "ofspaminvolvingnearlyidenticalmessagessenttonumerous",
                "byemail.Clickingonlinksinspamemailmaysendusersto",
                "byemail.Clickingonlinksinspamemailmaysendusersto",
                "phishingwebsitesorsitesthatarehostingmalware.",
                "Emailspam.Emailspam,alsoknownasjunkemailorbulkemail,",
                "isasubsetofspaminvolvingnearlyidenticalmessage",
                "ssenttonumerousbyemail.Clickingonlinksinspamemailmaysenduse",
                "rstophishingwebsitesorsitesthatarehostingmalware."
            ),
        ),
        (HTML_TEXT_STYLE_SCRIPT, "Thisisatest.Thisisatest."),
        (TEXT_ATTACHMENT, "Thisisatestmailing"),
        (TEXT_ATTACHMENT_W_NULL, "Thisisatestmailing"),
        (TEXT_ATTACHMENT_W_MULTIPLE_NULLS, "Thisisatestmailing"),
        (TEXT_ATTACHMENT_W_SUBJECT_NULL, "Thisisatestmailing"),
        (TEXT_ATTACHMENT_W_CONTENTTYPE_NULL, "Thisisatestmailing"),
    ] {
        assert_eq!(
            String::from_utf8(
                MessageParser::new()
                    .parse(input)
                    .unwrap()
                    .pyzor_digest(Vec::new())
                    .unwrap()
            )
            .unwrap(),
            expected,
            "failed for {input}"
        )
    }

    // Test SHA hash
    assert_eq!(
        MessageParser::new()
            .parse(HTML_TEXT_STYLE_SCRIPT)
            .unwrap()
            .pyzor_digest(Sha1::new())
            .unwrap()
            .finalize()
            .hex_encode(),
        "b2c27325a034c581df0c9ef37e4a0d63208a3e7e",
    )
}

const HTML_TEXT: &str = r#"MIME-Version: 1.0
Sender: chirila@gapps.spamexperts.com
Received: by 10.216.157.70 with HTTP; Thu, 16 Jan 2014 00:43:31 -0800 (PST)
Date: Thu, 16 Jan 2014 10:43:31 +0200
Delivered-To: chirila@gapps.spamexperts.com
X-Google-Sender-Auth: ybCmONS9U9D6ZUfjx-9_tY-hF2Q
Message-ID: <CAK-mJS8sE-V6qtspzzZ+bZ1eSUE_FNMt3K-5kBOG-z3NMgU_Rg@mail.gmail.com>
Subject: Test
From: Alexandru Chirila <chirila@spamexperts.com>
To: Alexandru Chirila <chirila@gapps.spamexperts.com>
Content-Type: multipart/alternative; boundary=001a11c25ff293069304f0126bfd

--001a11c25ff293069304f0126bfd
Content-Type: text/plain; charset=ISO-8859-1

Email spam.

Email spam, also known as junk email or unsolicited bulk email, is a subset
of electronic spam involving nearly identical messages sent to numerous
recipients by email. Clicking on links in spam email may send users to
phishing web sites or sites that are hosting malware.

--001a11c25ff293069304f0126bfd
Content-Type: text/html; charset=ISO-8859-1
Content-Transfer-Encoding: quoted-printable

<div dir=3D"ltr"><div>Email spam.</div><div><br></div><div>Email spam, also=
 known as junk email or unsolicited bulk email, is a subset of electronic s=
pam involving nearly identical messages sent to numerous recipients by emai=
l. Clicking on links in spam email may send users to phishing web sites or =
sites that are hosting malware.</div>
</div>

--001a11c25ff293069304f0126bfd--
"#;

const HTML_TEXT_STYLE_SCRIPT: &str = r#"MIME-Version: 1.0
Sender: chirila@gapps.spamexperts.com
Received: by 10.216.157.70 with HTTP; Thu, 16 Jan 2014 00:43:31 -0800 (PST)
Date: Thu, 16 Jan 2014 10:43:31 +0200
Delivered-To: chirila@gapps.spamexperts.com
X-Google-Sender-Auth: ybCmONS9U9D6ZUfjx-9_tY-hF2Q
Message-ID: <CAK-mJS8sE-V6qtspzzZ+bZ1eSUE_FNMt3K-5kBOG-z3NMgU_Rg@mail.gmail.com>
Subject: Test
From: Alexandru Chirila <chirila@spamexperts.com>
To: Alexandru Chirila <chirila@gapps.spamexperts.com>
Content-Type: multipart/alternative; boundary=001a11c25ff293069304f0126bfd

--001a11c25ff293069304f0126bfd
Content-Type: text/plain; charset=ISO-8859-1

This is a test.

--001a11c25ff293069304f0126bfd
Content-Type: text/html; charset=ISO-8859-1
Content-Transfer-Encoding: quoted-printable

<div dir=3D"ltr">
<style> This is my style.</style>
<script> This is my script.</script>
<div>This is a test.</div>
</div>

--001a11c25ff293069304f0126bfd--
"#;

const TEXT_ATTACHMENT: &str = r#"MIME-Version: 1.0
Received: by 10.76.127.40 with HTTP; Fri, 17 Jan 2014 02:21:43 -0800 (PST)
Date: Fri, 17 Jan 2014 12:21:43 +0200
Delivered-To: chirila.s.alexandru@gmail.com
Message-ID: <CALTHOsuHFaaatiXJKU=LdDCo4NmD_h49yvG2RDsWw17D0-NXJg@mail.gmail.com>
Subject: Test
From: Alexandru Chirila <chirila.s.alexandru@gmail.com>
To: Alexandru Chirila <chirila.s.alexandru@gmail.com>
Content-Type: multipart/mixed; boundary=f46d040a62c49bb1c804f027e8cc

--f46d040a62c49bb1c804f027e8cc
Content-Type: multipart/alternative; boundary=f46d040a62c49bb1c404f027e8ca

--f46d040a62c49bb1c404f027e8ca
Content-Type: text/plain; charset=ISO-8859-1

This is a test mailing

--f46d040a62c49bb1c404f027e8ca--
--f46d040a62c49bb1c804f027e8cc
Content-Type: image/png; name="tar.png"
Content-Disposition: attachment; filename="tar.png"
Content-Transfer-Encoding: base64
X-Attachment-Id: f_hqjas5ad0

iVBORw0KGgoAAAANSUhEUgAAAskAAADlCAAAAACErzVVAAAACXBIWXMAAAsTAAALEwEAmpwYAAAD
QmCC
--f46d040a62c49bb1c804f027e8cc--"#;

const TEXT_ATTACHMENT_W_NULL: &str = "MIME-Version: 1.0
Received: by 10.76.127.40 with HTTP; Fri, 17 Jan 2014 02:21:43 -0800 (PST)
Date: Fri, 17 Jan 2014 12:21:43 +0200
Delivered-To: chirila.s.alexandru@gmail.com
Message-ID: <CALTHOsuHFaaatiXJKU=LdDCo4NmD_h49yvG2RDsWw17D0-NXJg@mail.gmail.com>
Subject: Test
From: Alexandru Chirila <chirila.s.alexandru@gmail.com>
To: Alexandru Chirila <chirila.s.alexandru@gmail.com>
Content-Type: multipart/mixed; boundary=f46d040a62c49bb1c804f027e8cc

--f46d040a62c49bb1c804f027e8cc
Content-Type: multipart/alternative; boundary=f46d040a62c49bb1c404f027e8ca

--f46d040a62c49bb1c404f027e8ca
Content-Type: text/plain; charset=ISO-8859-1

This is a test ma\0iling
--f46d040a62c49bb1c804f027e8cc--";

const TEXT_ATTACHMENT_W_MULTIPLE_NULLS: &str = "MIME-Version: 1.0
Received: by 10.76.127.40 with HTTP; Fri, 17 Jan 2014 02:21:43 -0800 (PST)
Date: Fri, 17 Jan 2014 12:21:43 +0200
Delivered-To: chirila.s.alexandru@gmail.com
Message-ID: <CALTHOsuHFaaatiXJKU=LdDCo4NmD_h49yvG2RDsWw17D0-NXJg@mail.gmail.com>
Subject: Test
From: Alexandru Chirila <chirila.s.alexandru@gmail.com>
To: Alexandru Chirila <chirila.s.alexandru@gmail.com>
Content-Type: multipart/mixed; boundary=f46d040a62c49bb1c804f027e8cc

--f46d040a62c49bb1c804f027e8cc
Content-Type: multipart/alternative; boundary=f46d040a62c49bb1c404f027e8ca

--f46d040a62c49bb1c404f027e8ca
Content-Type: text/plain; charset=ISO-8859-1

This is a test ma\0\0\0iling
--f46d040a62c49bb1c804f027e8cc--";

const TEXT_ATTACHMENT_W_SUBJECT_NULL: &str = "MIME-Version: 1.0
Received: by 10.76.127.40 with HTTP; Fri, 17 Jan 2014 02:21:43 -0800 (PST)
Date: Fri, 17 Jan 2014 12:21:43 +0200
Delivered-To: chirila.s.alexandru@gmail.com
Message-ID: <CALTHOsuHFaaatiXJKU=LdDCo4NmD_h49yvG2RDsWw17D0-NXJg@mail.gmail.com>
Subject: Te\0\0\0st
From: Alexandru Chirila <chirila.s.alexandru@gmail.com>
To: Alexandru Chirila <chirila.s.alexandru@gmail.com>
Content-Type: multipart/mixed; boundary=f46d040a62c49bb1c804f027e8cc

--f46d040a62c49bb1c804f027e8cc
Content-Type: multipart/alternative; boundary=f46d040a62c49bb1c404f027e8ca

--f46d040a62c49bb1c404f027e8ca
Content-Type: text/plain; charset=ISO-8859-1

This is a test mailing
--f46d040a62c49bb1c804f027e8cc--";

const TEXT_ATTACHMENT_W_CONTENTTYPE_NULL: &str = "MIME-Version: 1.0
Received: by 10.76.127.40 with HTTP; Fri, 17 Jan 2014 02:21:43 -0800 (PST)
Date: Fri, 17 Jan 2014 12:21:43 +0200
Delivered-To: chirila.s.alexandru@gmail.com
Message-ID: <CALTHOsuHFaaatiXJKU=LdDCo4NmD_h49yvG2RDsWw17D0-NXJg@mail.gmail.com>
Subject: Test
From: Alexandru Chirila <chirila.s.alexandru@gmail.com>
To: Alexandru Chirila <chirila.s.alexandru@gmail.com>
Content-Type: multipart/mixed; boundary=f46d040a62c49bb1c804f027e8cc

--f46d040a62c49bb1c804f027e8cc
Content-Type: multipart/alternative; boundary=f46d040a62c49bb1c404f027e8ca

--f46d040a62c49bb1c404f027e8ca
Content-Type: text/plain; charset=\"iso-8859-1\0\0\0\"

This is a test mailing
--f46d040a62c49bb1c804f027e8cc--";

const HTML_RAW: &str = r#"<html><head><title>Email spam</title></head><body>
<p><b>Email spam</b>, also known as <b>junk email</b> 
or <b>unsolicited bulk email</b> (<i>UBE</i>), is a subset of 
<a href="/wiki/Spam_(electronic)" title="Spam (electronic)">electronic spam</a> 
involving nearly identical messages sent to numerous recipients by <a href="/wiki/Email" title="Email">
email</a>. Clicking on <a href="/wiki/Html_email#Security_vulnerabilities" title="Html email" class="mw-redirect">
links in spam email</a> may send users to <a href="/wiki/Phishing" title="Phishing">phishing</a> 
web sites or sites that are hosting <a href="/wiki/Malware" title="Malware">malware</a>.</body></html>"#;

const HTML_RAW_STRIPED: &str = concat!(
    "Email spam Email spam , also known as junk email or unsolicited bulk email ( UBE ),",
    " is a subset of electronic spam involving nearly identical messages sent to numerous recipients by email",
    " . Clicking on links in spam email may send users to phishing web sites or sites that are hosting malware ."
);
