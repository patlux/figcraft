//! Layer parameter regressions from upstream PR #675 (9daf39e), with the file-close case from
//! PR #608 (90d7521). Invalid explicit targets must not silently become defaults or selections.

use crate::params::id_of;
use crate::{EngineError, Session};
use figcraft_doc::*;
use serde_json::{Value, json};

fn top(s: &Session) -> Vec<String> {
    let doc = s.doc().unwrap();
    let page = s.page().unwrap();
    doc.node(page).unwrap().children.iter().filter_map(|c| doc.node(*c).map(|n| n.name.clone())).collect()
}

fn failed(r: std::result::Result<Value, EngineError>) -> Option<String> {
    match r {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    }
}

fn rect(parent: Value) -> Value {
    json!({"type": "rectangle", "name": "child", "parent": parent, "x": 0, "y": 0, "width": 10, "height": 10})
}

fn card(s: &mut Session) -> NodeId {
    let v = s.execute("node.create", &json!({"type": "frame", "name": "Card", "x": 0, "y": 0, "width": 100, "height": 100})).unwrap();
    id_of(&v["id"]).unwrap()
}

#[test]
fn a_parent_that_is_not_an_id_is_refused_rather_than_becoming_the_page() {
    let mut s = Session::new();
    let card = card(&mut s);
    for parent in [json!({"$step": 0}), json!("Card"), json!(-1), json!(true)] {
        let msg = failed(s.execute("node.create", &rect(parent.clone()))).unwrap_or_else(|| panic!("{parent} must be refused"));
        assert!(msg.contains("`parent` must be a node id"), "{parent}: {msg}");
    }
    assert_eq!(top(&s), ["Card"], "nothing landed on the page");
    let doc = s.doc().unwrap();
    assert!(doc.node(card).unwrap().children.is_empty(), "nor inside the frame");
}

#[test]
fn leaving_the_parent_out_or_null_still_means_the_page() {
    let mut s = Session::new();
    let card = card(&mut s);
    for parent in [None, Some(json!(null))] {
        let mut p = rect(json!(0));
        match parent {
            None => _ = p.as_object_mut().unwrap().remove("parent"),
            Some(v) => p["parent"] = v,
        }
        s.execute("node.create", &p).unwrap();
    }
    assert_eq!(top(&s), ["Card", "child", "child"]);
    let doc = s.doc().unwrap();
    assert!(doc.node(card).unwrap().children.is_empty());
}

#[test]
fn a_parent_id_that_names_nothing_is_still_reported() {
    let mut s = Session::new();
    card(&mut s);
    let msg = failed(s.execute("node.create", &rect(json!(99999)))).unwrap_or_else(|| panic!("a missing layer must fail"));
    assert!(msg.contains("no such node 99999"), "{msg}");
}

#[test]
fn an_index_that_is_not_a_whole_non_negative_number_is_refused() {
    let mut s = Session::new();
    let card = card(&mut s);
    // Keep upstream's lenient number convention: booleans still parse as 0/1.
    for index in [json!("last"), json!(1.5), json!(-1), json!(-0.5), json!(1e12)] {
        let mut p = rect(json!(card));
        p["index"] = index.clone();
        let msg = failed(s.execute("node.create", &p)).unwrap_or_else(|| panic!("index {index} must be refused"));
        assert!(msg.contains("`index`"), "{index}: {msg}");
    }
    let doc = s.doc().unwrap();
    assert!(doc.node(card).unwrap().children.is_empty(), "nothing was inserted");
}

#[test]
fn a_whole_index_still_inserts_where_it_says() {
    let mut s = Session::new();
    let card = card(&mut s);
    for name in ["a", "b"] {
        s.execute("node.create", &json!({"type": "rectangle", "name": name, "parent": card, "x": 0, "y": 0, "width": 10, "height": 10})).unwrap();
    }
    s.execute("node.create", &json!({"type": "rectangle", "name": "mid", "parent": card, "index": 1, "x": 0, "y": 0, "width": 10, "height": 10}))
        .unwrap();
    let doc = s.doc().unwrap();
    let names: Vec<&str> = doc.node(card).unwrap().children.iter().filter_map(|c| doc.node(*c).map(|n| n.name.as_str())).collect();
    assert_eq!(names, ["a", "mid", "b"]);
}

#[test]
fn ids_entries_that_are_not_ids_are_refused_instead_of_being_dropped() {
    let mut s = Session::new();
    let card = card(&mut s);
    for ids in [json!(["nope"]), json!(["nope", card]), json!([{"$step": 0}, card]), json!([null])] {
        let msg =
            failed(s.execute("node.set", &json!({"ids": ids, "props": {"name": "renamed"}}))).unwrap_or_else(|| panic!("{ids} must be refused"));
        assert!(msg.contains("`ids`"), "{ids}: {msg}");
    }
    assert_eq!(s.doc().unwrap().node(card).unwrap().name, "Card", "nothing was renamed");
}

#[test]
fn an_ids_that_is_not_an_array_is_refused_but_null_means_the_selection() {
    let mut s = Session::new();
    let card = card(&mut s);
    let msg =
        failed(s.execute("node.set", &json!({"ids": "nope", "props": {"name": "renamed"}}))).unwrap_or_else(|| panic!("a non-array ids must fail"));
    assert!(msg.contains("`ids` must be an array"), "{msg}");
    s.execute("select.set", &json!({"ids": [card]})).unwrap();
    s.execute("node.set", &json!({"ids": null, "props": {"name": "Selected"}})).unwrap();
    assert_eq!(s.doc().unwrap().node(card).unwrap().name, "Selected");
}

#[test]
fn a_single_id_that_is_not_an_id_is_refused() {
    let mut s = Session::new();
    card(&mut s);
    let msg =
        failed(s.execute("node.set", &json!({"id": {"$step": 0}, "props": {"name": "renamed"}}))).unwrap_or_else(|| panic!("a bad id must fail"));
    assert!(msg.contains("`id` must be a node id"), "{msg}");
}

#[test]
fn an_image_and_an_instance_parent_are_checked_the_same_way() {
    let mut s = Session::new();
    card(&mut s);
    let img = failed(s.execute("image.place", &json!({"data": "aGk=", "parent": {"$step": 0}})));
    assert!(img.unwrap_or_default().contains("`parent` must be a node id"), "image.place");
    let inst = failed(s.execute("instance.create", &json!({"component": 99999, "parent": {"$step": 0}})));
    assert!(inst.unwrap_or_default().contains("`parent` must be a node id"), "instance.create must validate the parent");
}

#[test]
fn file_close_rejects_a_bad_index_instead_of_closing_another_document() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    let docs = s.docs.len();
    for index in [json!(-1), json!(1.5), json!(99), json!("nope")] {
        let r = s.execute("file.close", &json!({"index": index, "force": true}));
        assert!(r.is_err(), "index {index} must be rejected, got {r:?}");
        assert_eq!(s.docs.len(), docs, "index {index} closed a document");
    }
}
