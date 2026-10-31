use super::css::{Tokens, css_decls};
use super::{CodeLang, Ctx, Document, LayoutMode, Node, NodeId, NodeKind, Param, Sizing, indent, num, with_v_if};

// ------------------------------------------------------------------ HTML and React markup

/// A word for the node's kind, naming layers whose name transliterates to nothing ("###" →
/// `.text`, `.frame`), instead of the anonymous `layer-`.
fn kind_word(n: &Node) -> &str {
    match &n.kind {
        NodeKind::Page { .. } => "page",
        NodeKind::Frame => "frame",
        NodeKind::Group => "group",
        NodeKind::Section { .. } => "section",
        NodeKind::Rectangle => "rectangle",
        NodeKind::Ellipse { .. } => "ellipse",
        NodeKind::Polygon { .. } => "polygon",
        NodeKind::Star { .. } => "star",
        NodeKind::Line => "line",
        NodeKind::Vector { .. } => "vector",
        NodeKind::Text(_) => "text",
        NodeKind::BooleanOp { .. } => "boolean",
        NodeKind::Component { .. } | NodeKind::ComponentSet { .. } => "component",
        NodeKind::Instance { .. } => "instance",
        NodeKind::Slice => "slice",
    }
}

/// A CSS class name from a layer name ("Card / Title" → "card-title"), unique in `used`. The name
/// is transliterated to ASCII first, so non-Latin names stay readable: 로그인 화면 →
/// `rogeuin-hwamyeon`, кнопка → `knopka`, über → `uber` (per-codepoint transliteration, the slug
/// libraries' way); a name left with nothing (symbols only) takes `kind`, and one starting with a
/// digit keeps the `layer-` prefix.
pub(crate) fn class_name(name: &str, kind: &str, used: &mut Vec<String>) -> String {
    let mut base = String::new();
    for c in deunicode::deunicode(name).chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            base.push(c);
        } else if !base.ends_with('-') && !base.is_empty() {
            base.push('-');
        }
    }
    // The 40-character cut lands on a word boundary when there is one past the first word.
    let base = if base.chars().count() > 40 {
        let cut: String = base.chars().take(40).collect();
        match cut.rfind('-') {
            Some(i) if i > 0 => cut[..i].to_string(),
            _ => cut,
        }
    } else {
        base
    };
    let base = base.trim_end_matches('-');
    let base = if base.is_empty() {
        kind.to_string()
    } else if base.starts_with(|c: char| c.is_ascii_digit()) {
        format!("layer-{base}")
    } else {
        base.to_string()
    };
    let name = (1..).map(|k| if k == 1 { base.clone() } else { format!("{base}-{k}") }).find(|n| !used.contains(n)).unwrap_or(base);
    used.push(name.clone());
    name
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "<br>")
}

/// Text as JSX children: HTML escapes, braces as expressions, line breaks as `<br />`.
fn jsx_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().take(10_000) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '{' => out.push_str("{\"{\"}"),
            '}' => out.push_str("{\"}\"}"),
            '\n' => out.push_str("<br />"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// The whole tree under `id` as nested HTML with a stylesheet: auto layout frames become
/// flexboxes, other children are placed absolutely in their parent, text becomes paragraphs.
pub(super) fn html(doc: &Document, id: NodeId) -> String {
    let ctx = Ctx::new(doc, id);
    let mut out = Markup::new(&ctx, CodeLang::Html);
    out.node(id, None, 0, "");
    format!("{}\n<style>\n{}{}</style>\n", out.markup, out.tokens.root_rule(), out.styles)
}

/// Markup being built for a layer tree: HTML (`class`), React JSX (`className`), Vue
/// (`{{ }}` interpolation, `v-if`, `:attr` bindings) or Leptos `view!` markup (text as Rust
/// string literals); the last three with component parameters and instances as calls. With its
/// stylesheet.
pub(crate) struct Markup<'c, 'a> {
    ctx: &'c Ctx<'a>,
    /// Html, React, Vue or Leptos.
    lang: CodeLang,
    used: Vec<String>,
    pub markup: String,
    pub styles: String,
    pub tokens: Tokens,
    /// The root's class (default: from its name) and extra JSX attributes.
    pub root_class: Option<String>,
    pub root_attrs: String,
    /// A Vue `v-if` guard for the element about to be written: Vue templates can't wrap an
    /// element in an expression, so a boolean parameter's layer carries the condition itself.
    v_if: Option<String>,
    /// A prefix for every generated class name (slot content joins the caller's stylesheet, where
    /// two slots' layer names may repeat).
    pub class_prefix: Option<String>,
    /// Each layer's path in the tree (names with sibling counts), class and declarations, so
    /// variants can be compared layer by layer.
    pub rules: Vec<(String, String, String)>,
    /// Instances as their own (overridden) layers instead of component calls, except those
    /// connected to code (page code).
    pub inline: bool,
    /// Auto layout frames and text as fluid boxes (`width: 100%` up to their width) for pages.
    pub fluid: bool,
    /// With `fluid`, other layers at least this share of the root's width are fluid too (a
    /// divider, a banner image); smaller ones (icons) keep their size. Default: none.
    pub fluid_leaves: f64,
    /// Inside an element that only takes phrasing content (a button, a heading).
    phrasing: bool,
    /// Page code's guessed `h1` (computed on first use).
    h1: Option<Option<NodeId>>,
    /// Inside a link or button (another one inside would be invalid HTML).
    interactive: bool,
    /// The layers links go to (computed on first use).
    targets: Option<Vec<NodeId>>,
}

/// How `n` sits in its parent: in the flex flow, or placed at its position.
fn placement_decls(n: &Node, parent: Option<&Node>, root: bool, in_flow: bool) -> String {
    let mut decls = String::new();
    if root {
        decls.push_str("position: relative;\n");
    } else if in_flow {
        let vertical = parent.and_then(|p| p.layout.as_ref()).is_some_and(|l| l.mode == LayoutMode::Vertical);
        let (main, cross) =
            if vertical { (n.child_layout.sizing_v, n.child_layout.sizing_h) } else { (n.child_layout.sizing_h, n.child_layout.sizing_v) };
        if main == Sizing::Fill {
            decls.push_str("flex: 1 1 0;\n");
        }
        if cross == Sizing::Fill {
            decls.push_str("align-self: stretch;\n");
        }
    } else {
        let c = n.transform.as_coeffs();
        decls.push_str(&format!("position: absolute;\nleft: {}px;\ntop: {}px;\n", num(c[4]), num(c[5])));
        let rot = figcraft_geom::rotation_degrees(n.transform);
        if rot.abs() > 0.01 {
            decls.push_str(&format!("transform: rotate({}deg);\ntransform-origin: 0 0;\n", num(-rot)));
        }
    }
    decls
}

impl<'c, 'a> Markup<'c, 'a> {
    pub fn new(ctx: &'c Ctx<'a>, lang: CodeLang) -> Self {
        Markup {
            ctx,
            lang,
            used: Vec::new(),
            markup: String::new(),
            styles: String::new(),
            tokens: Tokens::default(),
            root_class: None,
            root_attrs: String::new(),
            v_if: None,
            class_prefix: None,
            rules: Vec::new(),
            inline: false,
            fluid: false,
            fluid_leaves: f64::INFINITY,
            phrasing: false,
            h1: None,
            interactive: false,
            targets: None,
        }
    }

    /// JSX and Vue templates indent by two spaces, HTML and Leptos by four.
    fn pad(&self, depth: usize) -> String {
        if matches!(self.lang, CodeLang::React | CodeLang::Vue) { "  ".repeat(depth) } else { indent(depth) }
    }

    /// Component code (React or Leptos): parameters and instances as calls.
    fn code(&self) -> bool {
        self.lang != CodeLang::Html
    }

    /// A parameter's name in the generated code.
    fn ident(&self, p: &Param) -> String {
        if self.lang == CodeLang::Leptos { crate::component_codegen::rust_ident(&p.ident) } else { p.ident.clone() }
    }

    /// A layer, shown only while its boolean parameter is on (component code).
    pub fn node(&mut self, id: NodeId, parent: Option<&Node>, depth: usize, path: &str) {
        let Some(n) = self.ctx.doc.node(id) else { return };
        if self.code()
            && id != self.ctx.root
            && let Some(p) = self.ctx.param(n, "visible")
        {
            // A Vue template holds no arbitrary expressions, so the guard rides on the element.
            if self.lang == CodeLang::Vue {
                self.v_if = Some(self.ident(p));
                self.element(n, parent, depth, path);
                self.v_if = None;
                return;
            }
            let (pad, p) = (self.pad(depth), self.ident(p));
            let (open, close) =
                if self.lang == CodeLang::Leptos { (format!("{{{p}.then(|| view! {{"), "})}") } else { (format!("{{{p} && ("), ")}") };
            self.markup.push_str(&format!("{pad}{open}\n"));
            self.element(n, parent, depth + 1, path);
            self.markup.push_str(&format!("{pad}{close}\n"));
            return;
        }
        self.element(n, parent, depth, path);
    }

    fn rule(&mut self, path: &str, class: &str, decls: String) {
        self.styles.push_str(&format!(".{class} {{\n"));
        for line in decls.lines() {
            self.styles.push_str(&format!("  {line}\n"));
        }
        self.styles.push_str("}\n");
        self.rules.push((path.to_string(), class.to_string(), decls));
    }

    /// An instance as a call to its component (see `element`); true when it was written. `host` is
    /// the layer's semantics and anchor attribute (local fork, #748): a wrapper carries them.
    #[allow(clippy::too_many_arguments)]
    fn instance_call(
        &mut self,
        n: &Node,
        depth: usize,
        path: &str,
        class: &str,
        decls: &str,
        root: bool,
        (sem, id_attr): (&crate::semantics::Semantics, &str),
    ) -> bool {
        let pad = self.pad(depth);
        let attr = if self.lang == CodeLang::React { "className" } else { "class" };
        let connected = self.inline && crate::code_connect::link(self.ctx.doc, n.id, CodeLang::React).is_some();
        if self.code() && (!self.inline || connected) && !root && matches!(n.kind, NodeKind::Instance { .. }) {
            let at = depth + usize::from(!decls.is_empty());
            let run = if self.lang == CodeLang::Leptos { ".run()" } else { "" };
            let (inner, css) = match self.ctx.param(n, "mainComponent") {
                Some(p) if self.lang == CodeLang::Vue => (Some(format!("{}<component :is=\"{}\" />\n", self.pad(at), self.ident(p))), String::new()),
                Some(p) => (Some(format!("{}{{{}{run}}}\n", self.pad(at), self.ident(p))), String::new()),
                None => self.ctx.call(n, self.lang, at).map_or((None, String::new()), |(code, css)| (Some(code), css)),
            };
            self.styles.push_str(&css);
            if let Some(inner) = inner {
                // A Vue guard set by `node` rides on the call when there is no wrapper.
                let guard = if self.lang == CodeLang::Vue { self.v_if.take() } else { None };
                let host = *sem != crate::semantics::Semantics::default() || !id_attr.is_empty();
                if decls.is_empty() && !host {
                    self.markup.push_str(&with_v_if(&inner, guard.as_deref()));
                } else {
                    let guard = guard.map(|g| format!(" v-if=\"{g}\"")).unwrap_or_default();
                    let tag = if sem.tag.is_some() { sem.element(false) } else { "div" };
                    let attrs = format!("{id_attr}{}", self.sem_attrs(sem));
                    self.markup.push_str(&format!("{pad}<{tag} {attr}=\"{class}\"{guard}{attrs}>\n{inner}{pad}</{tag}>\n"));
                    let mut d = String::from(if self.fluid { "" } else { reset_for(sem.tag, false) });
                    d.push_str(decls);
                    self.rule(path, class, d);
                }
                return true;
            }
        }
        false
    }

    fn text_body(&self, n: &Node, t: &figcraft_doc::TextData) -> String {
        match (self.ctx.param(n, "characters").filter(|_| self.code()), self.lang) {
            (Some(p), CodeLang::Leptos) => format!("{{{}.clone()}}", self.ident(p)),
            (Some(p), CodeLang::Vue) => format!("{{{{ {} }}}}", self.ident(p)),
            (Some(p), _) => format!("{{{}}}", p.ident),
            (None, CodeLang::React) => jsx_text(&t.characters),
            // `view!` takes text as string literals and void elements self-closed.
            (None, CodeLang::Leptos) => {
                t.characters.chars().take(10_000).collect::<String>().lines().map(|l| format!("{l:?}")).collect::<Vec<_>>().join("<br/>")
            }
            // A Vue template reads `{{ }}` as interpolation, so braces are escaped.
            (None, CodeLang::Vue) => html_escape(&t.characters).replace('{', "&#123;").replace('}', "&#125;"),
            (None, _) => html_escape(&t.characters),
        }
    }

    /// The element semantics (local fork, upstream PR #748): a layer's kind, link or mark decides
    /// its tag and attributes, and a frame a link goes to carries the anchor the link names. Vue
    /// templates keep upstream's plain elements.
    fn semantics(&mut self, n: &Node) -> (crate::semantics::Semantics, String) {
        let doc = self.ctx.doc;
        let sem = if self.lang == CodeLang::Vue {
            crate::semantics::Semantics::default()
        } else {
            let mut sem = crate::semantics::of(doc, n);
            if self.fluid {
                // Page code: guess what nothing marked (the h1, landmarks and buttons by name).
                let h1 = *self.h1.get_or_insert_with(|| crate::semantics::page_h1(doc, self.ctx.root));
                crate::semantics::infer(n, h1 == Some(n.id), &mut sem);
            }
            // No link or button inside another: HTML forbids nested interactive content.
            if self.interactive && matches!(sem.tag, Some("a" | "button")) {
                sem.tag = None;
                sem.href = None;
            }
            sem
        };
        let targets = self.targets.get_or_insert_with(|| crate::semantics::link_targets(doc));
        let id_attr = if targets.binary_search_by_key(&n.id.0, |t| t.0).is_ok() {
            crate::semantics::anchor_for(doc, n.id).map(|a| format!(" id=\"{}\"", a.replace('"', ""))).unwrap_or_default()
        } else {
            String::new()
        };
        (sem, id_attr)
    }

    /// What the element's tag needs after the placement, before the layer's own look (local
    /// fork, #748).
    fn tag_decls(&self, n: &Node, sem: &crate::semantics::Semantics, decls: &mut String) {
        let text = matches!(n.kind, NodeKind::Text(_));
        // A frame without auto layout drawn as an element that is inline by default (a link, a
        // span inside a button) keeps its box: inline boxes ignore width and height.
        let inline = !text && n.layout.is_none() && (matches!(sem.tag, Some("a" | "span" | "label")) || (self.phrasing && sem.tag.is_none()));
        if inline {
            decls.push_str("display: block;\n");
        }
        // Tailwind's preflight already resets these for page code.
        if !self.fluid {
            decls.push_str("box-sizing: border-box;\n");
            decls.push_str(reset_for(sem.tag, text));
        }
    }

    /// The open tag's attributes: the root's variant attributes, the layer's anchor and semantics
    /// (local fork, #748) and a Vue `v-if` guard.
    fn open_attrs(&mut self, root: bool, id_attr: &str, sem: &crate::semantics::Semantics) -> String {
        let mut attrs = if root { self.root_attrs.clone() } else { String::new() };
        attrs.push_str(id_attr);
        attrs.push_str(&self.sem_attrs(sem));
        if self.lang == CodeLang::Vue
            && let Some(guard) = self.v_if.take()
        {
            attrs.push_str(&format!(" v-if=\"{guard}\""));
        }
        attrs
    }

    fn element(&mut self, n: &Node, parent: Option<&Node>, depth: usize, path: &str) {
        let doc = self.ctx.doc;
        let root = n.id == self.ctx.root;
        let class = match self.root_class.take().filter(|_| root) {
            Some(c) => {
                self.used.push(c.clone());
                c
            }
            None => {
                let class = class_name(&n.name, kind_word(n), &mut self.used);
                match &self.class_prefix {
                    Some(prefix) => format!("{prefix}{class}"),
                    None => class,
                }
            }
        };
        let pad = self.pad(depth);
        let attr = if self.lang == CodeLang::React { "className" } else { "class" };
        // How it sits in its parent: in the flex flow, or placed at its position.
        let in_flow = parent.is_some_and(|p| p.layout.is_some()) && !n.child_layout.absolute;
        let mut decls = placement_decls(n, parent, root, in_flow);
        let (sem, id_attr) = self.semantics(n);
        // Nested instances are calls (component code), in a wrapper that places them when needed
        // and carries what the instance is on the page (a link around a component, its anchor).
        // Page code inlines instances, except those connected to code (codeConnect.set): their
        // snippet, e.g. a design-system `<Button>`.
        if self.instance_call(n, depth, path, &class, &decls, root, (&sem, &id_attr)) {
            return;
        }
        let hugs = n.layout.is_some() || matches!(n.kind, NodeKind::Text(_));
        let hug = (hugs && n.child_layout.sizing_h == Sizing::Hug, hugs && n.child_layout.sizing_v == Sizing::Hug);
        let mut own = css_decls(doc, n, &mut self.tokens, hug);
        let page = self.ctx.doc.node(self.ctx.root).map_or(f64::INFINITY, |r| r.width);
        if self.fluid && (n.layout.is_some() || matches!(n.kind, NodeKind::Text(_)) || (!root && n.width >= page * self.fluid_leaves)) {
            // Full width, or sized by the flex row (`flex: 1`): no cap needed.
            own = fluid(&own, n.width >= page - 0.5 || decls.contains("flex: 1 1 0;"), n.layout.is_some());
        }
        if n.layout.is_none() && !n.children.is_empty() && !root && in_flow {
            own.push_str("position: relative;\n");
        }
        self.tag_decls(n, &sem, &mut decls);
        decls.push_str(&own);
        self.rule(path, &class, decls);
        let attrs = self.open_attrs(root, &id_attr, &sem);
        let open = |tag: &str| format!("{pad}<{tag} {attr}=\"{class}\"{attrs}>");
        match &n.kind {
            NodeKind::Text(t) => {
                let body = self.text_body(n, t);
                // Inside a button or heading only phrasing content: plain text is a span.
                let tag = if self.phrasing && sem.tag.is_none() { "span" } else { sem.element(true) };
                self.markup.push_str(&format!("{}{body}</{tag}>\n", open(tag)));
            }
            _ => {
                let kids = self.ctx.kids(n);
                let slot = self.ctx.param(n, "slot").filter(|_| self.code());
                let vue = self.lang == CodeLang::Vue;
                let leptos = self.lang == CodeLang::Leptos;
                let tag = if self.phrasing && sem.tag.is_none() { "span" } else { sem.element(false) };
                let open = open(tag);
                let slot_ident = slot.map(|p| match self.ctx.comp {
                    Some(c) => crate::component_codegen::slot_name(c, p),
                    None => class_name(&p.ident, "slot", &mut Vec::new()),
                });
                if let Some(p) = slot.filter(|_| kids.is_empty()) {
                    let content = if leptos {
                        format!("{{{}}}.as_ref().map(|c| c.run())", self.ident(p))
                    } else if vue {
                        // Vue's own slot element, named so an instance's content lands in the right one.
                        match slot_ident.clone() {
                            Some(name) => format!("<slot name=\"{name}\" />"),
                            None => "<slot />".into(),
                        }
                    } else {
                        format!("{{{}}}", p.ident)
                    };
                    self.markup.push_str(&format!("{open}{content}</{tag}>\n"));
                } else if kids.is_empty() || self.ctx.too_deep(depth) {
                    self.markup.push_str(&format!("{open}</{tag}>\n"));
                } else {
                    self.markup.push_str(&format!("{open}\n"));
                    // A slot shows the instance's content, or else the main's.
                    let (p1, p2) = (self.pad(depth + 1), self.pad(depth + 2));
                    let inner = match slot {
                        Some(_) if vue => {
                            let name = slot_ident.clone().map(|n| format!(" name=\"{n}\"")).unwrap_or_default();
                            self.markup.push_str(&format!("{p1}<slot{name}>\n"));
                            depth + 2
                        }
                        Some(p) if leptos => {
                            self.markup.push_str(&format!("{p1}{{match &{} {{ Some(c) => c.run(), None => view! {{\n", self.ident(p)));
                            depth + 2
                        }
                        Some(p) => {
                            self.markup.push_str(&format!("{p1}{{{} ?? (\n{p2}<>\n", p.ident));
                            depth + 3
                        }
                        None => depth + 1,
                    };
                    let mut seen: Vec<&str> = Vec::new();
                    let (outer, outer_interactive) = (self.phrasing, self.interactive);
                    self.phrasing = outer || sem.phrasing();
                    self.interactive = outer_interactive || matches!(sem.tag, Some("a" | "button"));
                    // Bottom layer first: later elements paint on top, like the layer order.
                    for k in kids {
                        let Some(kn) = doc.node(k) else { continue };
                        let count = seen.iter().filter(|s| **s == kn.name).count();
                        seen.push(&kn.name);
                        self.node(k, Some(n), inner, &format!("{path}/{}#{count}", kn.name));
                    }
                    self.phrasing = outer;
                    self.interactive = outer_interactive;
                    match slot {
                        Some(_) if vue => self.markup.push_str(&format!("{p1}</slot>\n")),
                        Some(_) if leptos => self.markup.push_str(&format!("{p1}}}.into_any() }}}}\n")),
                        Some(_) => self.markup.push_str(&format!("{p2}</>\n{p1})}}\n")),
                        None => {}
                    }
                    self.markup.push_str(&format!("{pad}</{tag}>\n"));
                }
            }
        }
    }

    /// The attributes `sem` adds (href, button type, image text, aria-label, hidden), quoted
    /// for the language: HTML attribute strings, JSX string expressions, Rust string literals.
    fn sem_attrs(&self, sem: &crate::semantics::Semantics) -> String {
        let quote = |v: &str| match self.lang {
            CodeLang::React => format!("{{{}}}", serde_json::Value::String(v.to_string())),
            CodeLang::Leptos => format!("{v:?}"),
            _ => format!("\"{}\"", v.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")),
        };
        let mut out = String::new();
        if let Some(h) = &sem.href {
            out.push_str(&format!(" href={}", quote(h)));
        }
        if sem.tag == Some("button") && sem.href.is_none() {
            out.push_str(" type=\"button\"");
        }
        // Links and buttons keep their native role and stay in the accessibility tree: an image
        // fill only names them, and `decorative` doesn't hide a focusable control.
        let control = matches!(sem.tag, Some("a" | "button"));
        if sem.decorative && !control {
            out.push_str(" aria-hidden=\"true\"");
        } else if let Some(alt) = sem.alt.as_ref().filter(|_| !sem.decorative) {
            if control {
                out.push_str(&format!(" aria-label={}", quote(alt)));
            } else {
                out.push_str(&format!(" role=\"img\" aria-label={}", quote(alt)));
            }
        } else if let Some(label) = &sem.label {
            out.push_str(&format!(" aria-label={}", quote(label)));
        }
        out
    }
}

/// Browser defaults the design doesn't have, put before a layer's own declarations so they win:
/// text has no margins, links and buttons look like their layer, lists have no bullets.
fn reset_for(tag: Option<&str>, text: bool) -> &'static str {
    match tag {
        Some("a") => "color: inherit;\ntext-decoration: none;\n",
        Some("button") => "border: none;\nbackground: none;\npadding: 0;\nfont: inherit;\ncolor: inherit;\ntext-align: inherit;\ncursor: pointer;\n",
        Some("ul" | "ol") => "margin: 0;\npadding: 0;\nlist-style: none;\n",
        Some("h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "label" | "p") => "margin: 0;\n",
        _ if text => "margin: 0;\n",
        _ => "",
    }
}

/// `decls` for a box that follows the page: a fixed width becomes `width: 100%` capped at that
/// width (or just the full width for boxes as wide as the page), and a frame's fixed height a
/// minimum, so content can wrap and grow.
// ponytail: width-based heuristic; fixed-size leaves (icons, images, shapes) keep their pixels.
fn fluid(decls: &str, full: bool, frame: bool) -> String {
    let mut out = String::with_capacity(decls.len() + 32);
    for line in decls.lines() {
        if let Some(w) = line.strip_prefix("width: ") {
            out.push_str("width: 100%;\n");
            if !full {
                out.push_str(&format!("max-width: {w}\n"));
            }
        } else if let Some(h) = line.strip_prefix("height: ").filter(|_| frame) {
            out.push_str(&format!("min-height: {h}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}
