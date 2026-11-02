/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::CalendarTemplateVariable;
use utils::template::Template;

const TEMPLATES: [(&str, &str, &str); 2] = [
    (
        "calendar-invite.html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/calendar-invite.html"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/calendar-invite.html.min"
        )),
    ),
    (
        "calendar-alarm.html",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/calendar-alarm.html"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/calendar-alarm.html.min"
        )),
    ),
];

// Every `{{...}}` token in a template, in order of appearance
fn tokens(contents: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut rest = contents;

    while let Some((_, after)) = rest.split_once("{{") {
        match after.split_once("}}") {
            Some((token, tail)) => {
                tokens.push(token.trim());
                rest = tail;
            }
            None => break,
        }
    }

    tokens
}

#[test]
fn shipped_calendar_templates_parse() {
    for (name, source, minified) in TEMPLATES {
        Template::<CalendarTemplateVariable>::parse(source)
            .unwrap_or_else(|err| panic!("{name} failed to parse: {err}"));
        Template::<CalendarTemplateVariable>::parse(minified)
            .unwrap_or_else(|err| panic!("{name}.min failed to parse: {err}"));
    }
}

#[test]
fn minified_calendar_templates_are_in_sync() {
    for (name, source, minified) in TEMPLATES {
        let source = tokens(source);
        assert!(source.len() > 10, "{name} yielded no tokens to compare");
        assert_eq!(
            source,
            tokens(minified),
            "{name}.min is stale, re-run resources/scripts/minify_html.sh"
        );
    }
}

#[test]
fn calendar_template_tokens_are_single_line() {
    // A newline inside `{{...}}` makes the parser reject the block
    for (name, source, minified) in TEMPLATES {
        for (suffix, contents) in [("", source), (".min", minified)] {
            for token in tokens(contents) {
                assert!(
                    !token.contains('\n') && !token.contains('\r'),
                    "{name}{suffix} has a multi-line token: {token:?}"
                );
            }
        }
    }
}
