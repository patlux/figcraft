//! Inner shadows must use their own blend mode, on shapes and on groups.
//! All scenes are synthetic. Hard-edge probes use independently calculated colours.
#![allow(clippy::unwrap_used)]

use figcraft_color::{BlendMode, Color, Paint};
use figcraft_doc::{Document, Effect, EffectKind, NodeId, NodeKind, Shadow};
use figcraft_render::{Options, Pixmap, View, render_view_single};

#[derive(Clone, Copy, Debug)]
enum Holder {
    Shape,
    Frame,
    Group,
}

const HOLDERS: [Holder; 3] = [Holder::Shape, Holder::Frame, Holder::Group];
const BASE: [u8; 3] = [40, 80, 120];

fn colour(rgb: [u8; 3], alpha: f32) -> Color {
    Color::new(f32::from(rgb[0]) / 255.0, f32::from(rgb[1]) / 255.0, f32::from(rgb[2]) / 255.0, alpha)
}

fn inner(blend: BlendMode, alpha: f32) -> Effect {
    Effect {
        kind: EffectKind::InnerShadow(Shadow {
            color: colour([128; 3], alpha),
            offset: (0.0, 12.0),
            radius: 0.0,
            spread: 0.0,
            blend,
            ..Shadow::default()
        }),
        visible: true,
    }
}

fn scene(holder: Holder, effects: Vec<Effect>, opacity: f32) -> (Document, NodeId, NodeId) {
    let mut doc = Document::new("Inner shadow blends");
    let page = doc.first_page().unwrap();
    let kind = match holder {
        Holder::Shape => NodeKind::Rectangle,
        Holder::Frame => NodeKind::Frame,
        Holder::Group => NodeKind::Group,
    };
    let subject = doc.create(page, kind, "Subject", 20.0, 20.0, 120.0, 80.0).unwrap();
    let content =
        if matches!(holder, Holder::Group) { doc.create(subject, NodeKind::Rectangle, "Content", 0.0, 0.0, 120.0, 80.0).unwrap() } else { subject };
    doc.get_mut(content).unwrap().fills = vec![Paint::solid(colour(BASE, 1.0))];
    let node = doc.get_mut(subject).unwrap();
    node.effects = effects;
    node.opacity = opacity;
    (doc, page, subject)
}

fn render(doc: &Document, page: NodeId, scale: f64) -> Pixmap {
    render_view_single(
        doc,
        page,
        &View { origin: (0.0, 0.0), scale, width: (160.0 * scale) as u32, height: (120.0 * scale) as u32 },
        &Options { background: Some(Color::WHITE), ..Options::default() },
    )
    .unwrap()
}

fn pixel(pix: &Pixmap, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * pix.width() + x) * 4) as usize;
    pix.data()[i..i + 4].try_into().unwrap()
}

fn near(got: [u8; 4], rgb: [u8; 3], why: &str) {
    let want = [rgb[0], rgb[1], rgb[2], 255];
    assert!(got.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 2), "{why}: got {got:?}, want {want:?}");
}

fn check_blends(holder: Holder) {
    // Opaque #808080 over #285078. Addition/subtraction, multiply and screen have
    // distinct expected colours. PlusDarker is not Multiply.
    for (blend, want) in [
        (BlendMode::Normal, [128, 128, 128]),
        (BlendMode::PlusLighter, [168, 208, 248]),
        (BlendMode::PlusDarker, [0, 0, 0]),
        (BlendMode::Multiply, [20, 40, 60]),
        (BlendMode::Screen, [148, 168, 188]),
    ] {
        let (doc, page, _) = scene(holder, vec![inner(blend, 1.0)], 1.0);
        for scale in [1.0, 2.0] {
            let pix = render(&doc, page, scale);
            let at = |x, y| pixel(&pix, (x as f64 * scale) as u32, (y as f64 * scale) as u32);
            near(at(80, 24), want, &format!("{holder:?}, {blend:?}, scale {scale}"));
            near(at(80, 60), BASE, "the centre has no shadow");
            near(at(80, 18), [255; 3], "an inner shadow stays inside");
        }
    }
}

#[test]
fn shape_inner_shadows_use_their_blend_mode() {
    check_blends(Holder::Shape);
    check_blends(Holder::Frame);
}

#[test]
fn group_inner_shadows_use_their_blend_mode() {
    check_blends(Holder::Group);
}

#[test]
fn shadow_alpha_and_layer_opacity_are_applied_once() {
    for holder in HOLDERS {
        for (blend, mixed) in
            [(BlendMode::Normal, [84u8, 104, 124]), (BlendMode::PlusLighter, [104, 144, 184]), (BlendMode::PlusDarker, [20, 40, 60])]
        {
            // Half-opacity shadow, then half-opacity layer over white.
            let want = mixed.map(|v| ((u16::from(v) + 255) / 2) as u8);
            let (doc, page, _) = scene(holder, vec![inner(blend, 0.5)], 0.5);
            near(pixel(&render(&doc, page, 1.0), 80, 24), want, &format!("{holder:?}, {blend:?}"));
        }
    }
}

#[test]
fn inner_shadows_keep_effect_order_and_ignore_hidden_effects() {
    for holder in HOLDERS {
        let mut hidden = inner(BlendMode::Normal, 1.0);
        hidden.visible = false;
        let add = inner(BlendMode::PlusLighter, 1.0);
        let subtract = inner(BlendMode::PlusDarker, 1.0);
        for (effects, want) in [(vec![add.clone(), hidden.clone(), subtract.clone()], [41, 81, 121]), (vec![subtract, hidden, add], [128, 128, 128])]
        {
            let (doc, page, _) = scene(holder, effects, 1.0);
            near(pixel(&render(&doc, page, 1.0), 80, 24), want, &format!("{holder:?}"));
        }
    }
}

#[test]
fn normal_inner_shadow_matches_the_previous_source_over_path() {
    for holder in HOLDERS {
        let shadow =
            Shadow { color: colour([128; 3], 0.6), offset: (0.0, 14.0), radius: 8.0, spread: -2.0, blend: BlendMode::Normal, ..Shadow::default() };
        let effect = Effect { kind: EffectKind::InnerShadow(shadow.clone()), visible: true };
        let (doc, page, subject) = scene(holder, vec![effect], 1.0);
        let mut reference = doc.clone();
        reference.get_mut(subject).unwrap().effects.clear();
        let mut plain = render(&reference, page, 1.0);
        let outline = figcraft_render::subtree_outline(&doc, doc.node(subject).unwrap(), 0).unwrap();
        let mask = figcraft_render::shadow::inner_shadow(&outline, 160, 120, doc.world_transform(subject), 1.0, &shadow).unwrap();
        // This is exactly the previous Normal compositing operation.
        plain.draw_pixmap(0, 0, mask.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        let got = render(&doc, page, 1.0);
        assert_eq!(got.data(), plain.data(), "{holder:?}: Normal must not change");
    }
}
