/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_split_filters_exhaustive() {
    let test_cases: Vec<(&str, Vec<SearchFilter>, Vec<SplitFilter>)> = vec![
        // Test 1: Operator followed by document set at depth 0
        (
            "Operator then document set at depth 0",
            vec![account_id(42), other_op("test"), doc_set(&[1, 2, 3])],
            vec![
                SplitFilter::External(vec![account_id(42), other_op("test")]),
                SplitFilter::Internal(doc_set(&[1, 2, 3])),
            ],
        ),
        // Test 2: Document set followed by operator at depth 0
        (
            "Document set then operator at depth 0",
            vec![account_id(42), doc_set(&[1, 2, 3]), other_op("test")],
            vec![
                SplitFilter::External(vec![account_id(42), other_op("test")]),
                SplitFilter::Internal(doc_set(&[1, 2, 3])),
            ],
        ),
        // Test 3: Multiple document sets with operator in between
        (
            "Multiple document sets at depth 0 with operator",
            vec![
                account_id(42),
                doc_set(&[1, 2]),
                other_op("middle"),
                doc_set(&[2, 4]),
            ],
            vec![
                SplitFilter::External(vec![account_id(42), other_op("middle")]),
                SplitFilter::Internal(doc_set(&[2])),
            ],
        ),
        // Test 4: Document set at depth 0, then AND group
        (
            "Document set then AND group",
            vec![
                account_id(42),
                doc_set(&[1, 2]),
                SearchFilter::And,
                other_op("a"),
                other_op("b"),
                SearchFilter::End,
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::And,
                    other_op("a"),
                    other_op("b"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 5: AND group followed by document set at depth 0
        (
            "AND group then document set",
            vec![
                account_id(42),
                SearchFilter::And,
                other_op("a"),
                other_op("b"),
                SearchFilter::End,
                doc_set(&[1, 2]),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::And,
                    other_op("a"),
                    other_op("b"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 6: Operator at depth 0, then OR group, then document set
        (
            "Operator, OR group, then document set",
            vec![
                account_id(42),
                other_op("pre"),
                SearchFilter::Or,
                other_op("a"),
                other_op("b"),
                SearchFilter::End,
                doc_set(&[1, 2, 3]),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Or,
                    other_op("a"),
                    other_op("b"),
                    SearchFilter::End,
                    other_op("pre"),
                ]),
                SplitFilter::Internal(doc_set(&[1, 2, 3])),
            ],
        ),
        // Test 7: Document set, OR group, operator
        (
            "Document set, OR group, operator",
            vec![
                account_id(42),
                doc_set(&[1, 2]),
                SearchFilter::Or,
                other_op("a"),
                other_op("b"),
                SearchFilter::End,
                other_op("post"),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Or,
                    other_op("a"),
                    other_op("b"),
                    SearchFilter::End,
                    other_op("post"),
                ]),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 8: Multiple OR branches with document sets between
        (
            "Multiple OR branches with document sets between",
            vec![
                account_id(42),
                SearchFilter::Or,
                other_op("a"),
                SearchFilter::End,
                doc_set(&[1, 2]),
                SearchFilter::Or,
                other_op("b"),
                SearchFilter::End,
                doc_set(&[1, 2, 5, 6]),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Or,
                    other_op("a"),
                    SearchFilter::End,
                    SearchFilter::Or,
                    other_op("b"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 9: Document sets at different depths - depth 0 and inside AND
        (
            "Document sets at different depths in AND",
            vec![
                account_id(42),
                doc_set(&[1, 2]),
                SearchFilter::And,
                other_op("a"),
                doc_set(&[2, 3]),
                SearchFilter::End,
            ],
            vec![
                SplitFilter::Internal(SearchFilter::And),
                SplitFilter::External(vec![account_id(42), other_op("a")]),
                SplitFilter::Internal(doc_set(&[2, 3])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 10: Operator, AND group with doc set inside, operator
        (
            "Operator, AND(operator, doc_set), operator",
            vec![
                account_id(42),
                other_op("pre"),
                SearchFilter::And,
                other_op("a"),
                doc_set(&[1, 2, 3]),
                SearchFilter::End,
                other_op("post"),
            ],
            vec![
                SplitFilter::Internal(SearchFilter::And),
                SplitFilter::External(vec![account_id(42), other_op("a")]),
                SplitFilter::Internal(doc_set(&[1, 2, 3])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::External(vec![account_id(42), other_op("pre"), other_op("post")]),
            ],
        ),
        // Test 11: Document set, nested groups, document set
        (
            "Doc set, AND(OR(a,b)), doc set",
            vec![
                account_id(42),
                SearchFilter::Or,
                doc_set(&[1, 2]),
                other_op("c"),
                SearchFilter::And,
                other_op("a"),
                other_op("b"),
                SearchFilter::End,
                doc_set(&[3, 4]),
                SearchFilter::End,
            ],
            vec![
                SplitFilter::Internal(SearchFilter::Or),
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Or,
                    SearchFilter::And,
                    other_op("a"),
                    other_op("b"),
                    SearchFilter::End,
                    other_op("c"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[1, 2, 3, 4])),
                SplitFilter::Internal(SearchFilter::End),
            ],
        ),
        // Test 12: OR with nested AND containing document sets, followed by operator
        (
            "OR(AND(doc_set, doc_set), operator) followed by operator",
            vec![
                account_id(42),
                SearchFilter::Or,
                SearchFilter::And,
                doc_set(&[1, 2]),
                doc_set(&[2, 3]),
                SearchFilter::End,
                other_op("b"),
                SearchFilter::End,
                other_op("post"),
            ],
            vec![
                SplitFilter::Internal(SearchFilter::Or),
                SplitFilter::Internal(SearchFilter::And),
                SplitFilter::Internal(doc_set(&[2])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::External(vec![account_id(42), other_op("b")]),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::External(vec![account_id(42), other_op("post")]),
            ],
        ),
        // Test 13: Complex: doc set, AND group, doc set, OR group, doc set
        (
            "Complex: doc, AND, doc, OR, doc",
            vec![
                account_id(42),
                doc_set(&[1, 2, 3]),
                SearchFilter::And,
                other_op("a"),
                SearchFilter::End,
                doc_set(&[1, 2, 3, 5]),
                SearchFilter::Or,
                other_op("b"),
                SearchFilter::End,
                doc_set(&[1, 2, 3, 6]),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::And,
                    other_op("a"),
                    SearchFilter::End,
                    SearchFilter::Or,
                    other_op("b"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[1, 2, 3])),
            ],
        ),
        // Test 14: Operator, NOT group, document set
        (
            "Operator, NOT(operator), document set",
            vec![
                account_id(42),
                other_op("pre"),
                SearchFilter::Not,
                other_op("a"),
                SearchFilter::End,
                doc_set(&[1, 2]),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Not,
                    other_op("a"),
                    SearchFilter::End,
                    other_op("pre"),
                ]),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 15: Document set, NOT group, operator
        (
            "Document set, NOT(operator), operator",
            vec![
                account_id(42),
                doc_set(&[1, 2]),
                SearchFilter::Not,
                other_op("a"),
                doc_set(&[3, 4]),
                SearchFilter::End,
                other_op("post"),
            ],
            vec![
                SplitFilter::Internal(SearchFilter::Not),
                SplitFilter::External(vec![account_id(42), other_op("a")]),
                SplitFilter::Internal(doc_set(&[3, 4])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::External(vec![account_id(42), other_op("post")]),
                SplitFilter::Internal(doc_set(&[1, 2])),
            ],
        ),
        // Test 16: Alternating doc sets and operators
        (
            "Alternating: doc, op, doc, op, doc",
            vec![
                account_id(42),
                doc_set(&[1]),
                other_op("a"),
                doc_set(&[1, 2]),
                other_op("b"),
                doc_set(&[1, 3]),
            ],
            vec![
                SplitFilter::External(vec![account_id(42), other_op("a"), other_op("b")]),
                SplitFilter::Internal(doc_set(&[1])),
            ],
        ),
        // Test 17: Multiple operators, then OR group with doc set inside, then doc set
        (
            "Multiple ops, OR(op, doc_set), doc",
            vec![
                account_id(42),
                other_op("a"),
                SearchFilter::Or,
                other_op("c"),
                doc_set(&[1, 2]),
                SearchFilter::End,
                other_op("b"),
                doc_set(&[3, 4]),
            ],
            vec![
                SplitFilter::Internal(SearchFilter::Or),
                SplitFilter::External(vec![account_id(42), other_op("c")]),
                SplitFilter::Internal(doc_set(&[1, 2])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::External(vec![account_id(42), other_op("a"), other_op("b")]),
                SplitFilter::Internal(doc_set(&[3, 4])),
            ],
        ),
        // Test 18: Doc set before and after nested OR(AND(op))
        (
            "Doc, OR(AND(op)), doc",
            vec![
                account_id(42),
                doc_set(&[1]),
                SearchFilter::Or,
                SearchFilter::And,
                other_op("a"),
                other_op("c"),
                SearchFilter::End,
                other_op("b"),
                SearchFilter::End,
                doc_set(&[2]),
            ],
            vec![
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Or,
                    SearchFilter::And,
                    other_op("a"),
                    other_op("c"),
                    SearchFilter::End,
                    other_op("b"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[])),
            ],
        ),
        // Test 19: AND group with doc set, operator between, OR group with doc set
        (
            "AND(op, doc), op, OR(op, doc)",
            vec![
                account_id(42),
                SearchFilter::And,
                other_op("a"),
                doc_set(&[1, 2]),
                SearchFilter::End,
                other_op("middle"),
                SearchFilter::Or,
                other_op("b"),
                other_op("c"),
                doc_set(&[3, 4]),
                SearchFilter::End,
            ],
            vec![
                SplitFilter::Internal(SearchFilter::And),
                SplitFilter::External(vec![account_id(42), other_op("a")]),
                SplitFilter::Internal(doc_set(&[1, 2])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::Internal(SearchFilter::Or),
                SplitFilter::External(vec![
                    account_id(42),
                    SearchFilter::Or,
                    other_op("b"),
                    other_op("c"),
                    SearchFilter::End,
                ]),
                SplitFilter::Internal(doc_set(&[3, 4])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::External(vec![account_id(42), other_op("middle")]),
            ],
        ),
        // Test 20: Deep nesting with document sets at multiple levels
        (
            "Deep nesting: doc, AND(doc, OR(doc, AND(op, doc)))",
            vec![
                account_id(42),
                doc_set(&[1]),
                SearchFilter::And,
                doc_set(&[2]),
                SearchFilter::Or,
                doc_set(&[3]),
                SearchFilter::And,
                other_op("a"),
                doc_set(&[4]),
                SearchFilter::End,
                SearchFilter::End,
                SearchFilter::End,
            ],
            vec![
                SplitFilter::Internal(SearchFilter::And),
                SplitFilter::Internal(SearchFilter::Or),
                SplitFilter::Internal(SearchFilter::And),
                SplitFilter::External(vec![account_id(42), other_op("a")]),
                SplitFilter::Internal(doc_set(&[4])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::Internal(doc_set(&[3])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::Internal(doc_set(&[2])),
                SplitFilter::Internal(SearchFilter::End),
                SplitFilter::Internal(doc_set(&[1])),
            ],
        ),
    ];

    for (description, input, expected) in test_cases {
        println!("------ Running test: {} ------", description);
        let result = split_filters(input.clone());
        assert!(result.is_some(), "Test '{}' returned None", description);

        let result = result.unwrap();
        if result != expected {
            print_split_filter_code(&result);
        }
        assert_eq!(result, expected, "Test '{description}' failed",);
    }
}

fn account_id(id: u64) -> SearchFilter {
    SearchFilter::Integer {
        field: SearchField::AccountId,
        op: Ordering::Equal,
        value: id,
    }
}

fn other_op(value: &str) -> SearchFilter {
    SearchFilter::Text {
        field: SearchField::DocumentId,
        op: TextMatch::Exact,
        value: value.to_string(),
        language: Language::None,
    }
}

fn doc_set(ids: &[u32]) -> SearchFilter {
    let mut bitmap = RoaringBitmap::new();
    for id in ids {
        bitmap.insert(*id);
    }
    SearchFilter::DocumentSet(bitmap)
}

fn print_split_filter_code(splits: &[SplitFilter]) {
    println!("vec![");
    for split in splits {
        match split {
            SplitFilter::Internal(filter) => {
                print!("    SplitFilter::Internal(");
                print_search_filter_code(filter, 0);
                println!("),");
            }
            SplitFilter::External(filters) => {
                println!("    SplitFilter::External(vec![");
                for filter in filters {
                    print!("        ");
                    print_search_filter_code(filter, 2);
                    println!(",");
                }
                println!("    ]),");
            }
        }
    }
    println!("]");
}

fn print_search_filter_code(filter: &SearchFilter, indent_level: usize) {
    let indent = "    ".repeat(indent_level);
    match filter {
        SearchFilter::Integer {
            field: SearchField::AccountId,
            op: Ordering::Equal,
            value,
        } => {
            print!("account_id({})", value);
        }
        SearchFilter::Text {
            field: SearchField::DocumentId,
            op: TextMatch::Exact,
            value,
            ..
        } => {
            print!("other_op(\"{}\")", value);
        }
        SearchFilter::Text { .. }
        | SearchFilter::Integer { .. }
        | SearchFilter::KeyValue { .. } => {
            print!("{}{:?}", indent, filter);
        }
        SearchFilter::DocumentSet(bitmap) => {
            let ids: Vec<u32> = bitmap.iter().collect();
            if ids.is_empty() {
                print!("doc_set(&[])");
            } else if ids.len() <= 5 {
                print!("doc_set(&[");
                for (i, id) in ids.iter().enumerate() {
                    if i > 0 {
                        print!(", ");
                    }
                    print!("{}", id);
                }
                print!("])");
            } else {
                // For large bitmaps, create inline
                println!("{{");
                println!("{}    let mut bitmap = RoaringBitmap::new();", indent);
                for id in ids {
                    println!("{}    bitmap.insert({});", indent, id);
                }
                print!("{}    doc_set_bitmap(bitmap)", indent);
                println!();
                print!("{}}}", indent);
            }
        }
        SearchFilter::And => print!("SearchFilter::And"),
        SearchFilter::Or => print!("SearchFilter::Or"),
        SearchFilter::Not => print!("SearchFilter::Not"),
        SearchFilter::End => print!("SearchFilter::End"),
    }
}
