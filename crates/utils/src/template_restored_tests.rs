/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_simple_variable_substitution() {
    let template = Template::parse("Hello {{name}}!").unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("name".to_string(), "World".to_string());

    let result = template.eval(&vars);
    assert_eq!(result, "Hello World!");
}

#[test]
fn test_multiple_variables() {
    let template = Template::parse("{{greeting}} {{name}}, today is {{day}}").unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("greeting".to_string(), "Hello".to_string());
    vars.insert_single("name".to_string(), "Alice".to_string());
    vars.insert_single("day".to_string(), "Monday".to_string());

    let result = template.eval(&vars);
    assert_eq!(result, "Hello Alice, today is Monday");
}

#[test]
fn test_missing_variable() {
    let template = Template::parse("Hello {{name}}!").unwrap();
    let vars = Variables::<String, String>::new();

    let result = template.eval(&vars);
    assert_eq!(result, "Hello !");
}

#[test]
fn test_static_text_only() {
    let template = Template::parse("This is just static text").unwrap();
    let vars = Variables::<String, String>::new();

    let result = template.eval(&vars);
    assert_eq!(result, "This is just static text");
}

#[test]
fn test_empty_template() {
    let template = Template::parse("").unwrap();
    let vars = Variables::<String, String>::new();

    let result = template.eval(&vars);
    assert_eq!(result, "");
}

#[test]
fn test_if_block_with_existing_variable() {
    let template = Template::parse("{{#if show_message}}Hello World!{{/if show_message}}").unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("show_message".to_string(), "true".to_string());

    let result = template.eval(&vars);
    assert_eq!(result, "Hello World!");
}

#[test]
fn test_if_block_with_missing_variable() {
    let template = Template::parse("{{#if show_message}}Hello World!{{/if show_message}}").unwrap();
    let vars = Variables::<String, String>::new();

    let result = template.eval(&vars);
    assert_eq!(result, "");
}

#[test]
fn test_if_block_with_content_and_variables() {
    let template = Template::parse(
        "{{#if notifications}}You have notifications: {{count}}{{/if notifications}}",
    )
    .unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("notifications".to_string(), "true".to_string());
    vars.insert_single("count".to_string(), "5".to_string());

    let result = template.eval(&vars);
    assert_eq!(result, "You have notifications: 5");
}

#[test]
fn test_foreach_block_basic() {
    let template = Template::parse("{{#each items}}{{name}} {{/each items}}").unwrap();
    let mut vars = Variables::<String, String>::new();

    let items = vec![
        vec![("name".to_string(), "Item1".to_string())],
        vec![("name".to_string(), "Item2".to_string())],
        vec![("name".to_string(), "Item3".to_string())],
    ];
    vars.insert_block("items".to_string(), items);

    let result = template.eval(&vars);
    assert_eq!(result, "Item1 Item2 Item3 ");
}

#[test]
fn test_foreach_block_multiple_variables() {
    let template =
        Template::parse("{{#each notifications}}* {{name}} at {{time}}\n{{/each notifications}}")
            .unwrap();
    let mut vars = Variables::<String, String>::new();

    let notifications = vec![
        vec![
            ("name".to_string(), "Meeting".to_string()),
            ("time".to_string(), "10:00".to_string()),
        ],
        vec![
            ("name".to_string(), "Call".to_string()),
            ("time".to_string(), "14:30".to_string()),
        ],
    ];
    vars.insert_block("notifications".to_string(), notifications);

    let result = template.eval(&vars);
    assert_eq!(result, "* Meeting at 10:00\n* Call at 14:30\n");
}

#[test]
fn test_foreach_block_empty() {
    let template = Template::parse("{{#each items}}{{name}}{{/each items}}").unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_block("items".to_string(), Vec::<Vec<(String, String)>>::new());

    let result = template.eval(&vars);
    assert_eq!(result, "");
}

#[test]
fn test_foreach_block_missing_variable() {
    let template = Template::parse("{{#each items}}{{name}}{{/each items}}").unwrap();
    let vars = Variables::<String, String>::new();

    let result = template.eval(&vars);
    assert_eq!(result, "");
}

#[test]
fn test_complex_template_example() {
    let template_str = r#"Hello {{name}},

{{#if notifications}}You have the following notifications:
{{#each notifications}}* {{name}} at {{time}}
{{/each notifications}}{{/if notifications}}
Best regards"#;

    let template = Template::parse(template_str).unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("name".to_string(), "Alice".to_string());
    vars.insert_single("notifications".to_string(), "true".to_string());

    let notifications = vec![
        vec![
            ("name".to_string(), "Team Meeting".to_string()),
            ("time".to_string(), "09:00".to_string()),
        ],
        vec![
            ("name".to_string(), "Doctor Appointment".to_string()),
            ("time".to_string(), "15:30".to_string()),
        ],
    ];
    vars.insert_block("notifications".to_string(), notifications);

    let result = template.eval(&vars);
    let expected = r#"Hello Alice,

You have the following notifications:
* Team Meeting at 09:00
* Doctor Appointment at 15:30

Best regards"#;

    assert_eq!(result, expected);
}

#[test]
fn test_complex_template_no_notifications() {
    let template_str = r#"Hello {{name}},

{{#if notifications}}
You have the following notifications:
{{#each notifications}}
* {{name}} at {{time}}
{{/each notifications}}{{/if notifications}}
Best regards"#;

    let template = Template::parse(template_str).unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("name".to_string(), "Bob".to_string());

    let result = template.eval(&vars);
    let expected = r#"Hello Bob,


Best regards"#;

    assert_eq!(result, expected);
}

#[test]
fn test_whitespace_handling() {
    let template = Template::parse("{{ name }}").unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("name".to_string(), "Test".to_string());

    let result = template.eval(&vars);
    assert_eq!(result, "Test");
}

#[test]
fn test_whitespace_in_blocks() {
    let template = Template::parse("{{# if condition }}Content{{/ if condition }}").unwrap();
    let mut vars = Variables::<String, String>::new();
    vars.insert_single("condition".to_string(), "true".to_string());

    let result = template.eval(&vars);
    assert_eq!(result, "Content");
}

// Error handling tests
#[test]
fn test_unmatched_opening_brace() {
    let result = Template::<String>::parse("Hello {{name");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unmatched {{"));
}

#[test]
fn test_invalid_block_start() {
    let result = Template::<String>::parse("{{#invalid block}}{{/invalid block}}");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid block start"));
}

#[test]
fn test_invalid_block_end() {
    let result = Template::<String>::parse("{{#if test}}{{\\/invalid block}}");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Unmatched"));
}

#[test]
fn test_mismatched_block_names() {
    let result = Template::<String>::parse("{{#if test}}{{/if different}}");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .contains("Block end does not match start")
    );
}

#[test]
fn test_mismatched_block_types() {
    let result = Template::<String>::parse("{{#if test}}{{/each test}}");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .contains("Block end does not match start")
    );
}

#[test]
fn test_consecutive_braces() {
    let template = Template::parse("{{}}").unwrap();
    let vars = Variables::<String, String>::new();

    let result = template.eval(&vars);
    assert_eq!(result, "");
}

#[test]
fn test_foreach_with_missing_inner_variables() {
    let template = Template::parse("{{#each items}}{{name}}: {{missing}}{{/each items}}").unwrap();
    let mut vars = Variables::<String, String>::new();

    let items = vec![
        vec![("name".to_string(), "Item1".to_string())],
        vec![("name".to_string(), "Item2".to_string())],
    ];
    vars.insert_block("items".to_string(), items);

    let result = template.eval(&vars);
    assert_eq!(result, "Item1: Item2: ");
}

/*#[test]
fn test_full() {
    // Load static html in memory from resources/email-templates/calendar-alarm.html
    let template_str = include_str!("../../../resources/email-templates/calendar-alarm.html");
    let template: Template<CalendarTemplateVariable> = Template::parse(template_str).unwrap();

    let mut vars = Variables::<CalendarTemplateVariable, String>::new();
    vars.insert_single(
        CalendarTemplateVariable::PageTitle,
        "Test Event".to_string(),
    );
    vars.insert_single(CalendarTemplateVariable::Header, "Event Header".to_string());
    vars.insert_single(CalendarTemplateVariable::Footer, "Event Footer".to_string());
    vars.insert_single(
        CalendarTemplateVariable::EventTitle,
        "Meeting with Team".to_string(),
    );
    vars.insert_single(
        CalendarTemplateVariable::EventDescription,
        "Discuss project updates".to_string(),
    );
    vars.insert_single(
        CalendarTemplateVariable::EventDetails,
        "Details about the event".to_string(),
    );
    vars.insert_single(
        CalendarTemplateVariable::ActionUrl,
        "http://example.com/action".to_string(),
    );
    vars.insert_single(
        CalendarTemplateVariable::ActionName,
        "Join Meeting".to_string(),
    );
    vars.insert_single(
        CalendarTemplateVariable::AttendeesTitle,
        "Attendees".to_string(),
    );
    vars.insert_block(
        CalendarTemplateVariable::EventDetails,
        vec![
            vec![
                (CalendarTemplateVariable::Key, "Location".to_string()),
                (
                    CalendarTemplateVariable::Value,
                    "Conference Room A".to_string(),
                ),
            ],
            vec![
                (CalendarTemplateVariable::Key, "Time".to_string()),
                (
                    CalendarTemplateVariable::Value,
                    "10:00 AM - 11:00 AM".to_string(),
                ),
            ],
        ],
    );
    vars.insert_block(
        CalendarTemplateVariable::Attendees,
        vec![
            vec![
                (CalendarTemplateVariable::Key, "Alice".to_string()),
                (
                    CalendarTemplateVariable::Value,
                    "alice@domain.org".to_string(),
                ),
            ],
            vec![
                (CalendarTemplateVariable::Key, "Bob".to_string()),
                (
                    CalendarTemplateVariable::Value,
                    "bob@domain.org".to_string(),
                ),
            ],
        ],
    );
    let result = template.eval(&vars);
    // Write result to test.html
    std::fs::write("test.html", result).expect("Unable to write file");
}*/
