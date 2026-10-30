/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_html_to_tokens_text() {
    let input = "Hello, world!";
    let tokens = html_to_tokens(input);
    assert_eq!(
        tokens,
        vec![HtmlToken::Text {
            text: "Hello, world!".into()
        }]
    );
}

#[test]
fn test_html_to_tokens_start_tag() {
    let input = "<div>";
    let tokens = html_to_tokens(input);
    assert_eq!(
        tokens,
        vec![HtmlToken::StartTag {
            name: 7760228,
            attributes: vec![],
            is_self_closing: false
        }]
    );
}

#[test]
fn test_html_to_tokens_end_tag() {
    let input = "</div>";
    let tokens = html_to_tokens(input);
    assert_eq!(tokens, vec![HtmlToken::EndTag { name: 7760228 }]);
}

#[test]
fn test_html_to_tokens_comment() {
    let input = "<!-- This is a comment -->";
    let tokens = html_to_tokens(input);
    assert_eq!(
        tokens,
        vec![HtmlToken::Comment {
            text: "!-- This is a comment --".into()
        }]
    );
}

#[test]
fn test_html_to_tokens_mixed() {
    let input = "<div>Hello, <span>&quot; world &quot; </span>!</div>";
    let tokens = html_to_tokens(input);
    assert_eq!(
        tokens,
        vec![
            HtmlToken::StartTag {
                name: 7760228,
                attributes: vec![],
                is_self_closing: false
            },
            HtmlToken::Text {
                text: "Hello,".into()
            },
            HtmlToken::StartTag {
                name: 1851879539,
                attributes: vec![],
                is_self_closing: false
            },
            HtmlToken::Text {
                text: " \" world \"".into()
            },
            HtmlToken::EndTag { name: 1851879539 },
            HtmlToken::Text { text: " !".into() },
            HtmlToken::EndTag { name: 7760228 }
        ]
    );
}

#[test]
fn test_html_to_tokens_with_attributes() {
    let input = r#"<input type="text" value="test"><single/><one attr/><a b=1 b c="123">"#;
    let tokens = html_to_tokens(input);
    assert_eq!(
        tokens,
        vec![
            HtmlToken::StartTag {
                name: 500186508905,
                attributes: vec![
                    (1701869940, Some("text".into())),
                    (435761734006, Some("test".into()))
                ],
                is_self_closing: false
            },
            HtmlToken::StartTag {
                name: 111516266162547,
                attributes: vec![],
                is_self_closing: true
            },
            HtmlToken::StartTag {
                name: 6647407,
                attributes: vec![(1920234593, None)],
                is_self_closing: true
            },
            HtmlToken::StartTag {
                name: 97,
                attributes: vec![(98, Some("1".into())), (98, None), (99, Some("123".into()))],
                is_self_closing: false
            }
        ]
    );
}
