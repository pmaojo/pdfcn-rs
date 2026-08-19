use pdfcn_parser::{parse_document, AttrValue, Node};

#[test]
fn parses_tag_with_classes_id_and_attrs() {
    let src = "%table.w-full#invoice(role=\"table\")\n  %tr\n    %td Hello";
    let doc = parse_document(src).expect("should parse");
    assert_eq!(doc.len(), 1);
    match &doc[0] {
        Node::Element {
            tag,
            id,
            classes,
            attrs,
            children,
        } => {
            assert_eq!(tag, "table");
            assert_eq!(id.as_deref(), Some("invoice"));
            assert_eq!(classes, &vec!["w-full".to_string()]);
            assert_eq!(attrs[0].name, "role");
            assert_eq!(attrs[0].value, AttrValue::Literal("table".to_string()));
            assert_eq!(children.len(), 1);
        }
        other => panic!("expected Element, got {other:?}"),
    }
}

#[test]
fn parses_component_tag() {
    let src = "%InvoiceTable(rows={{ items }})";
    let doc = parse_document(src).unwrap();
    match &doc[0] {
        Node::Component { name, attrs, .. } => {
            assert_eq!(name, "InvoiceTable");
            assert_eq!(attrs[0].name, "rows");
            assert_eq!(attrs[0].value, AttrValue::Expr("items".to_string()));
        }
        other => panic!("expected Component, got {other:?}"),
    }
}

#[test]
fn parses_for_and_if_directives() {
    let src = "- for item in items\n  %li {{ item.name }}\n- if active\n  %span Active\n- else\n  %span Inactive";
    let doc = parse_document(src).unwrap();
    assert!(matches!(doc[0], Node::For { .. }));
    assert!(matches!(doc[1], Node::If { .. }));
    if let Node::For {
        binding, iterable, ..
    } = &doc[0]
    {
        assert_eq!(binding, "item");
        assert_eq!(iterable, "items");
    }
    if let Node::If {
        branches,
        else_body,
    } = &doc[1]
    {
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].0, "active");
        assert!(else_body.is_some());
    }
}

#[test]
fn parses_implicit_div_and_include() {
    let src = ".card#summary\n  - include \"partials/footer.haml\"";
    let doc = parse_document(src).unwrap();
    match &doc[0] {
        Node::Element {
            tag, id, classes, ..
        } => {
            assert_eq!(tag, "div");
            assert_eq!(id.as_deref(), Some("summary"));
            assert_eq!(classes, &vec!["card".to_string()]);
        }
        other => panic!("expected Element, got {other:?}"),
    }
}

#[test]
fn rejects_tab_indentation() {
    let src = "%div\n\t%span hi";
    assert!(parse_document(src).is_err());
}
