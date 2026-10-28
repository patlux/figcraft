use super::*;

impl Session {
    /// A session with one new document.
    pub fn new() -> Self {
        let mut s = Self::empty();
        s.docs.push(DocState::new(Document::new("Untitled"), None));
        s
    }

    /// A session with no documents (the home screen).
    pub fn empty() -> Self {
        Self {
            docs: Vec::new(),
            active: 0,
            clipboard: Clipboard::default(),
            prefs: Prefs::default(),
            tool: tools::ToolState::default(),
            ui_requests: Vec::new(),
            ui_dark: None,
            journal: Vec::new(),
            group_depth: 0,
            closed_in_group: Vec::new(),
            recent_colors: Vec::new(),
            last_command: None,
            message: None,
            libraries: library::Cache::default(),
            plugins: plugins::Runtime::default(),
            live: live::Lives::default(),
            background: background::Background::default(),
        }
    }

    /// The background a new file's page gets: the dark page colour while the interface is dark.
    pub fn new_page_background(&self) -> figcraft_color::Color {
        if self.ui_dark == Some(true) { figcraft_color::Color::DARK_PAGE } else { figcraft_color::Color::DEFAULT_PAGE }
    }

    /// Tell the engine the interface's theme (the UI does, every frame). A page still on its
    /// theme-given colour follows the theme, at creation and when it changes; a page whose colour
    /// was chosen keeps it (local fork, upstream PR #659). A file nothing was done to stays clean,
    /// an edited one stays as dirty as the user's own edits left it.
    pub fn set_ui_dark(&mut self, dark: bool) {
        if self.ui_dark == Some(dark) {
            return;
        }
        self.ui_dark = Some(dark);
        let (to, from) = if dark {
            (figcraft_color::Color::DARK_PAGE, figcraft_color::Color::DEFAULT_PAGE)
        } else {
            (figcraft_color::Color::DEFAULT_PAGE, figcraft_color::Color::DARK_PAGE)
        };
        let repaint = |doc: &mut figcraft_doc::Document| {
            let mut any = false;
            for p in doc.pages.clone() {
                if let Some(figcraft_doc::NodeKind::Page { background, .. }) = doc.node_mut(p).map(|n| &mut n.kind)
                    && *background == from
                {
                    *background = to;
                    any = true;
                }
            }
            any
        };
        for ds in &mut self.docs {
            let clean = Arc::ptr_eq(&ds.doc, &ds.saved_doc);
            let mut doc = (*ds.doc).clone();
            if !repaint(&mut doc) {
                continue;
            }
            let doc = Arc::new(doc);
            if clean {
                ds.saved_doc = doc.clone();
            } else {
                let mut saved = (*ds.saved_doc).clone();
                repaint(&mut saved);
                ds.saved_doc = Arc::new(saved);
            }
            ds.doc = doc;
            ds.revision += 1;
        }
    }

    pub fn with_document(doc: Document) -> Self {
        let mut s = Self::empty();
        s.docs.push(DocState::new(doc, None));
        s
    }

    /// Files new to the session start with the View toggles the preferences keep.
    pub fn apply_view_prefs(&mut self) {
        for ds in self.docs.iter_mut().filter(|d| !d.view_from_prefs) {
            cmd::view::apply_kept(&self.prefs, &mut ds.view);
            ds.view_from_prefs = true;
        }
    }
}
