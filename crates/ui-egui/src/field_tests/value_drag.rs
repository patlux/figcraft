//! Fork (#256, upstream PR #285 closed): a number field's value dragged sideways scrubs it, like
//! its label, at the label's rates and in one undo step; a still click types, a drag that sets off
//! up or down scrolls the panel, and Mixed values and the variable button don't drag. Upstream's
//! Alt-held scrub on the field (`alt_held_over_a_field_scrubs_it`) works alongside: Alt as the drag
//! begins makes it a scrub at a step a point whichever way it sets off.
//!
//! These tests came in with the patch (e291d94) and were lost in the 0.11.2 merge (94994db); they
//! are back, on upstream's current field ids and helpers.

use egui::accesskit::Role;
use egui::{CursorIcon, Event, Id, Key, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
use figcraft_engine::Session;
use serde_json::json;

use super::{click, field, geom, id, key, setup, undo_steps};
use crate::layers_drag_tests::Ui;

/// Press at `from`, move by `d` in small steps with `modifiers` held, then let go.
fn drag_by(u: &mut Ui, from: Pos2, d: Vec2, modifiers: Modifiers) {
    let press = |pressed, pos| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
    u.frame_with(vec![Event::PointerMoved(from), press(true, from)], modifiers);
    let steps = 10;
    for k in 1..=steps {
        u.frame_with(vec![Event::PointerMoved(from + d * k as f32 / steps as f32)], modifiers);
    }
    let to = from + d;
    u.frame_with(vec![Event::PointerMoved(to), press(false, to)], modifiers);
    u.frame(vec![]);
}

/// Field `name`'s value area (where it is typed and dragged).
fn value_rect(u: &mut Ui, name: &str) -> Rect {
    let (p, _) = field(u, name);
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update.expect("an AccessKit update").nodes;
    let n = nodes
        .iter()
        .map(|(_, n)| n)
        .filter(|n| n.role() == Role::SpinButton && n.label() == Some(name))
        .min_by(|a, b| a.bounds().map_or(f64::INFINITY, |r| r.y0).total_cmp(&b.bounds().map_or(f64::INFINITY, |r| r.y0)))
        .unwrap();
    let b = n.bounds().unwrap();
    let r = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
    assert!(r.contains(p));
    r
}

/// The cursor the window asks for with the pointer resting at `p`.
fn cursor_at(u: &mut Ui, p: Pos2) -> CursorIcon {
    u.frame(vec![Event::PointerMoved(p)]);
    u.frame_output(vec![Event::PointerMoved(p)]).platform_output.cursor_icon
}

fn typing(u: &Ui, field_id: &str) -> bool {
    u.ctx.data(|d| d.get_temp::<String>(Id::unique(field_id).with("editing"))).is_some()
}

#[test]
fn dragging_the_value_scrubs_it_like_the_label_in_one_undo_step() {
    let (mut u, a, _) = setup();
    let shift = Modifiers { shift: true, ..Modifiers::NONE };
    let alt = Modifiers { alt: true, ..Modifiers::NONE };
    // (points, modifiers, x after): X starts at 10. Shift is ten steps a point; Alt held as the
    // drag begins is upstream's Alt scrub on the field, a step a point (not the label's tenths).
    for (dx, m, want) in [(25.0, Modifiers::NONE, 35.0), (10.0, shift, 135.0), (20.0, alt, 155.0), (-37.0, Modifiers::NONE, 118.0)] {
        let steps = undo_steps(&u);
        let (p, _) = field(&mut u, "X");
        drag_by(&mut u, p, vec2(dx, 0.0), m);
        assert_eq!(geom(&u, a).0, want, "X's value dragged {dx} points with {m:?}");
        assert_eq!(undo_steps(&u), steps + 1, "one undo step for the whole drag");
        assert!(!typing(&u, "pos_x"), "a drag doesn't start typing");
    }
    u.app.run("edit.undo", json!({}));
    assert_eq!(geom(&u, a).0, 155.0, "undo takes back the last drag alone");
    let (p, _) = field(&mut u, "Opacity");
    drag_by(&mut u, p, vec2(-30.0, 0.0), Modifiers::NONE);
    assert_eq!(u.app.session.doc().unwrap().node(a).unwrap().opacity, 0.7);
    // The value says it can be dragged; not while it is being typed.
    let (p, _) = field(&mut u, "Y");
    assert_eq!(cursor_at(&mut u, p), CursorIcon::ResizeHorizontal);
    click(&mut u, p);
    assert!(typing(&u, "pos_y"), "a still click on the value types");
    assert_ne!(cursor_at(&mut u, p), CursorIcon::ResizeHorizontal);
    u.frame(vec![Event::Text("42".into())]);
    key(&mut u, Key::Enter, Modifiers::NONE);
    assert_eq!(geom(&u, a).1, 42.0);
}

#[test]
fn a_drag_inside_the_text_being_typed_selects_it() {
    let (mut u, a, _) = setup();
    let r = value_rect(&mut u, "X");
    click(&mut u, r.center());
    assert!(typing(&u, "pos_x"));
    // A second click puts the caret in the text; a drag across it then selects.
    let from = pos2(r.min.x + 2.0, r.center().y);
    click(&mut u, from);
    let steps = undo_steps(&u);
    drag_by(&mut u, from, vec2(20.0, 0.0), Modifiers::NONE);
    let state = egui::TextEdit::load_state(&u.ctx, Id::unique("pos_x").with("te")).expect("the text being typed");
    let range = state.cursor.char_range().expect("a selection");
    assert_ne!(range.primary.index, range.secondary.index, "the drag selected text");
    assert_eq!((geom(&u, a).0, undo_steps(&u)), (10.0, steps), "and scrubbed nothing");
    // A drag that begins in the field's edge above the text doesn't scrub either.
    drag_by(&mut u, pos2(r.center().x, r.min.y + 1.0), vec2(25.0, 0.0), Modifiers::NONE);
    assert_eq!(geom(&u, a).0, 10.0);
}

#[test]
fn a_drag_up_or_down_the_value_scrolls_the_panel_instead() {
    let (mut u, a, _) = setup();
    // Enough paints and effects for the Design panel to scroll.
    for _ in 0..10 {
        u.app.run("fill.add", json!({"ids": [a]}));
        u.app.run("effect.add", json!({"ids": [a], "type": "dropShadow"}));
    }
    let r = value_rect(&mut u, "Opacity");
    let steps = undo_steps(&u);
    drag_by(&mut u, r.center(), vec2(2.0, -150.0), Modifiers::NONE);
    assert_eq!((u.app.session.doc().unwrap().node(a).unwrap().opacity, undo_steps(&u)), (1.0, steps), "nothing scrubbed");
    let moved = value_rect(&mut u, "Opacity");
    assert!(moved.min.y < r.min.y - 50.0, "the panel scrolled: {r:?} → {moved:?}");
}

#[test]
fn mixed_values_and_the_variable_button_are_not_dragged() {
    let (mut u, a, b) = setup();
    // The Opacity field can take a variable: its button sits at the value's right end.
    let r = value_rect(&mut u, "Opacity");
    let bind = pos2(r.max.x - 11.0, r.center().y);
    assert_ne!(cursor_at(&mut u, bind), CursorIcon::ResizeHorizontal);
    let steps = undo_steps(&u);
    drag_by(&mut u, bind, vec2(-30.0, 0.0), Modifiers::NONE);
    assert_eq!((u.app.session.doc().unwrap().node(a).unwrap().opacity, undo_steps(&u)), (1.0, steps), "a drag from the button");
    click(&mut u, bind);
    assert!(crate::variable_ui::picker_open(&u.ctx), "a click on it still opens the variables");
    u.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    // Mixed has no value to drag from: the drag changes nothing and a click still types.
    u.app.run("select.set", json!({"ids": [a, b]}));
    let (p, shown) = field(&mut u, "X");
    assert_eq!(shown, "Mixed");
    assert_ne!(cursor_at(&mut u, p), CursorIcon::ResizeHorizontal);
    let before = (geom(&u, a), geom(&u, b), undo_steps(&u));
    drag_by(&mut u, p, vec2(30.0, 0.0), Modifiers::NONE);
    assert_eq!((geom(&u, a), geom(&u, b), undo_steps(&u)), before);
    click(&mut u, p);
    assert!(typing(&u, "pos_x"));
}

#[test]
fn dragging_colour_picker_and_prototype_fields_is_one_undo_step() {
    // The colour picker's RGB fields: live while dragged, one undo step.
    let (mut u, a, _) = setup();
    let red = |u: &Ui| u.app.session.doc().unwrap().node(a).unwrap().fills[0].solid_color().unwrap().to_rgba8()[0];
    let r0 = red(&u);
    u.ctx.data_mut(|d| d.insert_temp(Id::unique("cp_model"), 1usize));
    u.app.ui.picker = Some(crate::colorpicker::PickerTarget::Paint { fills: true, index: 0, ids: vec![a] });
    // Settle sizing and the anchored position before reading field hit rectangles.
    for _ in 0..5 {
        u.frame(vec![]);
    }
    let f = u.ctx.read_response(Id::unique(("cp_f", "r")).with("field")).expect("the R field").rect;
    let steps = undo_steps(&u);
    drag_by(&mut u, f.center(), vec2(-20.0, 0.0), Modifiers::NONE);
    assert_eq!((red(&u), undo_steps(&u)), (r0 - 20, steps + 1));
    // A prototype delay: set once, when let go.
    let mut s = Session::new();
    let f1 = id(s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 200, "height": 300})).unwrap());
    let f2 = id(s.execute("frame.create", &json!({"x": 300, "y": 0, "width": 200, "height": 300})).unwrap());
    let it = json!({"id": f1, "trigger": {"type": "afterDelay", "ms": 500}, "action": {"type": "navigate", "dest": f2}});
    s.execute("proto.interaction.add", &it).unwrap();
    s.execute("select.set", &json!({"ids": [f1]})).unwrap();
    let mut u = Ui::new(s);
    u.app.ui.right_tab = crate::RightTab::Prototype;
    crate::proto_panel::open_editor(&u.ctx, Some((f1, 0)));
    for _ in 0..3 {
        u.frame(vec![]);
    }
    let f = u.ctx.read_response(Id::unique(("proto_delay", f1, 0usize)).with("field")).expect("the delay field").rect;
    let steps = undo_steps(&u);
    drag_by(&mut u, f.center(), vec2(30.0, 0.0), Modifiers::NONE);
    let ms = match u.app.session.doc().unwrap().node(f1).unwrap().interactions[0].trigger {
        figcraft_doc::proto::Trigger::AfterDelay { ms } => ms,
        _ => panic!("still after a delay"),
    };
    assert_eq!((ms, undo_steps(&u)), (800, steps + 1));
}

#[test]
fn dragging_the_scale_panel_width_scales_live_in_one_undo_step() {
    let (mut u, a, _) = setup();
    u.app.run("tool.set", json!({"tool": "scale"}));
    u.frame(vec![]);
    let f = u.ctx.read_response(Id::unique("scale_w").with("field")).expect("the Scale panel's W").rect;
    let steps = undo_steps(&u);
    drag_by(&mut u, f.center(), vec2(20.0, 0.0), Modifiers::NONE);
    let (_, _, w, h) = geom(&u, a);
    assert_eq!((w, h, undo_steps(&u)), (60.0, 45.0, steps + 1), "40×30 scaled to a width of 60");
}
