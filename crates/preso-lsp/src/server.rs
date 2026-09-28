//! The protocol loop: handshake, document sync, request dispatch.
//!
//! Synchronous and single-threaded: every request is answered from the
//! in-memory document before the next is read. A deck is a few hundred
//! lines and every feature is a linear scan, so there is nothing worth
//! cancelling or parallelising.

use crate::convert::{Encoding, LineIndex, uri_to_path};
use crate::features::{self, Doc};
use anyhow::Result;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentLinkRequest, DocumentSymbolRequest, FoldingRangeRequest,
    HoverRequest, Request as _,
};
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionProviderCapability, CompletionOptions,
    DocumentLinkOptions, FoldingRangeProviderCapability, HoverProviderCapability, InitializeParams,
    InitializeResult, OneOf, PositionEncodingKind, PublishDiagnosticsParams, ServerCapabilities,
    ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Uri,
};
use preso_core::edit::Action;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Characters after which an editor should ask for completions without
/// waiting to be prompted: directive openers, `key=`, and path starts.
const TRIGGERS: &[&str] = &["-", ":", "=", "(", "/", " "];

pub fn run(connection: &Connection) -> Result<()> {
    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let encoding = negotiate_encoding(&params);
    let snippets = params
        .capabilities
        .text_document
        .as_ref()
        .and_then(|t| t.completion.as_ref())
        .and_then(|c| c.completion_item.as_ref())
        .and_then(|i| i.snippet_support)
        .unwrap_or(false);
    let result = InitializeResult {
        capabilities: capabilities(encoding),
        server_info: Some(ServerInfo {
            name: "preso-lsp".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };
    connection.initialize_finish(id, serde_json::to_value(result)?)?;
    tracing::info!(?encoding, snippets, "initialized");

    let mut server = Server {
        connection,
        docs: HashMap::new(),
        media_dirs: HashMap::new(),
        deck_used: HashMap::new(),
        encoding,
        snippets,
        diagrams: crate::diagrams::Diagrams::default(),
    };
    for message in &connection.receiver {
        match message {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                server.request(req)?;
            }
            Message::Notification(note) => server.notification(note)?,
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// UTF-8 when the client can count in it (cheaper both ways), else the
/// protocol's UTF-16.
fn negotiate_encoding(params: &InitializeParams) -> Encoding {
    let offered = params
        .capabilities
        .general
        .as_ref()
        .and_then(|g| g.position_encodings.as_ref());
    if offered.is_some_and(|e| e.contains(&PositionEncodingKind::UTF8)) {
        Encoding::Utf8
    } else {
        Encoding::Utf16
    }
}

fn capabilities(encoding: Encoding) -> ServerCapabilities {
    let mut kinds = vec![CodeActionKind::QUICKFIX, CodeActionKind::REFACTOR];
    kinds.extend(
        Action::ALL
            .iter()
            .map(|a| CodeActionKind::from(a.kind().to_string())),
    );
    // Styles have a kind per preset; advertise their groups.
    kinds.extend(
        [
            "refactor.preso.image",
            "refactor.preso.table",
            "refactor.preso.code",
        ]
        .map(CodeActionKind::new),
    );
    ServerCapabilities {
        position_encoding: Some(match encoding {
            Encoding::Utf8 => PositionEncodingKind::UTF8,
            Encoding::Utf16 => PositionEncodingKind::UTF16,
        }),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                // Saves refresh what the whole deck references.
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        )),
        document_symbol_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(kinds),
            ..CodeActionOptions::default()
        })),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(TRIGGERS.iter().map(|t| t.to_string()).collect()),
            ..CompletionOptions::default()
        }),
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        ..ServerCapabilities::default()
    }
}

struct Server<'c> {
    connection: &'c Connection,
    docs: HashMap<Uri, String>,
    /// Each open document's asset directory, found when it's opened.
    media_dirs: HashMap<Uri, Option<PathBuf>>,
    /// Each open document's deck-wide references, refreshed on open and
    /// save (walking every chapter per keystroke would be wasteful; the
    /// document's own references are always current).
    deck_used: HashMap<Uri, HashSet<PathBuf>>,
    encoding: Encoding,
    snippets: bool,
    /// Diagrams drawn to check their zoom labels, kept between keystrokes.
    diagrams: crate::diagrams::Diagrams,
}

impl Server<'_> {
    fn notification(&mut self, note: Notification) -> Result<()> {
        match note.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let p: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(note.params)?;
                let uri = p.text_document.uri;
                self.docs.insert(uri.clone(), p.text_document.text);
                let media_dir = uri_to_path(&uri).and_then(|p| crate::roots::media_dir(&p));
                self.media_dirs.insert(uri.clone(), media_dir);
                self.refresh_deck_references();
                self.publish(&uri)?;
            }
            DidChangeTextDocument::METHOD => {
                let p: lsp_types::DidChangeTextDocumentParams =
                    serde_json::from_value(note.params)?;
                // Full sync: the last change holds the whole document.
                if let Some(change) = p.content_changes.into_iter().last() {
                    let uri = p.text_document.uri;
                    self.docs.insert(uri.clone(), change.text);
                    self.publish(&uri)?;
                }
            }
            DidSaveTextDocument::METHOD => self.refresh_deck_references(),
            DidCloseTextDocument::METHOD => {
                let p: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(note.params)?;
                self.docs.remove(&p.text_document.uri);
                self.media_dirs.remove(&p.text_document.uri);
                self.deck_used.remove(&p.text_document.uri);
                self.send_diagnostics(p.text_document.uri, Vec::new())?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Recompute every open document's deck-wide references, reading open
    /// documents from their buffers.
    fn refresh_deck_references(&mut self) {
        let open: HashMap<PathBuf, &str> = self
            .docs
            .iter()
            .filter_map(|(uri, text)| Some((uri_to_path(uri)?.canonicalize().ok()?, text.as_str())))
            .collect();
        let read = |path: &std::path::Path| open.get(path).map(|t| t.to_string());
        self.deck_used = self
            .docs
            .keys()
            .filter_map(|uri| {
                let path = uri_to_path(uri)?;
                Some((uri.clone(), crate::roots::deck_references(&path, read)))
            })
            .collect();
    }

    fn request(&self, req: Request) -> Result<()> {
        let id = req.id.clone();
        let response = match req.method.as_str() {
            DocumentSymbolRequest::METHOD => self.handle::<DocumentSymbolRequest>(req, |doc, _| {
                Some(lsp_types::DocumentSymbolResponse::Nested(
                    features::symbols(doc),
                ))
            }),
            FoldingRangeRequest::METHOD => {
                self.handle::<FoldingRangeRequest>(req, |doc, _| Some(features::folding(doc)))
            }
            CodeActionRequest::METHOD => self.handle::<CodeActionRequest>(req, |doc, p| {
                let analysis = preso_core::lint::check(doc.text);
                let only = p.context.only.as_deref();
                Some(features::code_actions(doc, &analysis, p.range, only))
            }),
            Completion::METHOD => self.handle::<Completion>(req, |doc, p| {
                let pos = p.text_document_position.position;
                let col = doc.index.byte_col(pos);
                let items = features::completions(doc, pos.line as usize, col, self.snippets);
                Some(lsp_types::CompletionResponse::Array(items))
            }),
            HoverRequest::METHOD => self.handle::<HoverRequest>(req, |doc, p| {
                let pos = p.text_document_position_params.position;
                let col = doc.index.byte_col(pos);
                features::hover(doc, pos.line as usize, col)
            }),
            DocumentLinkRequest::METHOD => self.handle::<DocumentLinkRequest>(req, |doc, _| {
                let analysis = preso_core::lint::check(doc.text);
                Some(features::links(doc, &analysis))
            }),
            method => {
                tracing::debug!(method, "unhandled request");
                Response::new_err(id, ErrorCode::MethodNotFound as i32, method.to_string())
            }
        };
        self.connection.sender.send(response.into())?;
        Ok(())
    }

    /// Decode `R`'s params, run `f` against the document they name, and
    /// wrap the result (`null` for a document that isn't open).
    fn handle<R>(&self, req: Request, f: impl FnOnce(&Doc, R::Params) -> R::Result) -> Response
    where
        R: lsp_types::request::Request,
        R::Params: HasUri,
    {
        let params: R::Params = match serde_json::from_value(req.params) {
            Ok(p) => p,
            Err(e) => {
                return Response::new_err(req.id, ErrorCode::InvalidParams as i32, e.to_string());
            }
        };
        let uri = params.uri().clone();
        let Some(text) = self.docs.get(&uri) else {
            return Response::new_ok(req.id, serde_json::Value::Null);
        };
        let dir = deck_dir(&uri);
        let doc = Doc {
            uri: &uri,
            text,
            dir: dir.as_deref(),
            media_dir: self.media_dirs.get(&uri).and_then(Option::as_deref),
            deck_used: self.deck_used.get(&uri),
            index: LineIndex::new(text, self.encoding),
            diagrams: &self.diagrams,
        };
        Response::new_ok(req.id, f(&doc, params))
    }

    fn publish(&self, uri: &Uri) -> Result<()> {
        let Some(text) = self.docs.get(uri) else {
            return Ok(());
        };
        let dir = deck_dir(uri);
        let doc = Doc {
            uri,
            text,
            dir: dir.as_deref(),
            media_dir: self.media_dirs.get(uri).and_then(Option::as_deref),
            deck_used: self.deck_used.get(uri),
            index: LineIndex::new(text, self.encoding),
            diagrams: &self.diagrams,
        };
        let analysis = preso_core::lint::check(text);
        let diagnostics = features::diagnostics(&doc, &analysis);
        self.send_diagnostics(uri.clone(), diagnostics)
    }

    fn send_diagnostics(&self, uri: Uri, diagnostics: Vec<lsp_types::Diagnostic>) -> Result<()> {
        let params = PublishDiagnosticsParams {
            uri,
            diagnostics,
            version: None,
        };
        let note = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
        self.connection.sender.send(note.into())?;
        Ok(())
    }
}

/// The directory a document's relative paths resolve against.
fn deck_dir(uri: &Uri) -> Option<PathBuf> {
    uri_to_path(uri)?.parent().map(PathBuf::from)
}

/// Request params that name a document.
trait HasUri {
    fn uri(&self) -> &Uri;
}

impl HasUri for lsp_types::DocumentSymbolParams {
    fn uri(&self) -> &Uri {
        &self.text_document.uri
    }
}

impl HasUri for lsp_types::FoldingRangeParams {
    fn uri(&self) -> &Uri {
        &self.text_document.uri
    }
}

impl HasUri for lsp_types::CodeActionParams {
    fn uri(&self) -> &Uri {
        &self.text_document.uri
    }
}

impl HasUri for lsp_types::CompletionParams {
    fn uri(&self) -> &Uri {
        &self.text_document_position.text_document.uri
    }
}

impl HasUri for lsp_types::HoverParams {
    fn uri(&self) -> &Uri {
        &self.text_document_position_params.text_document.uri
    }
}

impl HasUri for lsp_types::DocumentLinkParams {
    fn uri(&self) -> &Uri {
        &self.text_document.uri
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::path::Path;

    /// An editor on the other end of an in-memory connection.
    struct Client {
        conn: Connection,
        server: Option<std::thread::JoinHandle<()>>,
        next_id: i32,
    }

    impl Client {
        fn start(capabilities: Value) -> Client {
            let (server, conn) = Connection::memory();
            let handle = std::thread::spawn(move || run(&server).unwrap());
            let mut client = Client {
                conn,
                server: Some(handle),
                next_id: 1,
            };
            let init = client.request("initialize", json!({ "capabilities": capabilities }));
            assert_eq!(init["serverInfo"]["name"], "preso-lsp");
            client.notify("initialized", json!({}));
            client
        }

        fn notify(&self, method: &str, params: Value) {
            let note = Notification::new(method.to_string(), params);
            self.conn.sender.send(note.into()).unwrap();
        }

        fn request(&mut self, method: &str, params: Value) -> Value {
            let id = self.next_id;
            self.next_id += 1;
            let req = Request::new(id.into(), method.to_string(), params);
            self.conn.sender.send(req.into()).unwrap();
            loop {
                match self.recv() {
                    Message::Response(r) if r.id == id.into() => match r.response_result {
                        Ok(result) => return result,
                        Err(e) => panic!("{method}: {e:?}"),
                    },
                    _ => continue,
                }
            }
        }

        fn diagnostics(&self) -> Value {
            loop {
                if let Message::Notification(n) = self.recv()
                    && n.method == PublishDiagnostics::METHOD
                {
                    return n.params["diagnostics"].clone();
                }
            }
        }

        fn recv(&self) -> Message {
            self.conn
                .receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("server replies")
        }

        fn open(&self, uri: &str, text: &str) {
            self.notify(
                "textDocument/didOpen",
                json!({ "textDocument": {
                    "uri": uri, "languageId": "markdown", "version": 1, "text": text
                }}),
            );
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            let id = self.next_id;
            let req = Request::new(id.into(), "shutdown".to_string(), Value::Null);
            let _ = self.conn.sender.send(req.into());
            self.notify("exit", Value::Null);
            if let Some(handle) = self.server.take()
                && !std::thread::panicking()
            {
                handle.join().unwrap();
            }
        }
    }

    fn deck_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("preso-lsp-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("img")).unwrap();
        std::fs::write(dir.join("img/there.png"), b"").unwrap();
        std::fs::write(dir.join("img/notes.txt"), b"").unwrap();
        dir
    }

    fn uri_of(path: &Path) -> String {
        crate::convert::path_to_uri(path)
            .unwrap()
            .as_str()
            .to_string()
    }

    const DECK: &str = "\
---
title: Demo
---

# Intro

![ok](img/there.png)
![gone](img/missing.png)

---

<!-- layuot: TwoColumn -->
# Two

left

right
";

    fn codes(diagnostics: &Value) -> Vec<&str> {
        diagnostics
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn hover_themes_and_new_checks() {
        let dir = deck_dir("hover");
        std::fs::create_dir_all(dir.join("themes")).unwrap();
        std::fs::write(dir.join("themes/house.toml"), "").unwrap();
        let uri = uri_of(&dir.join("talk.md"));
        let deck = "\
---
theme: themes/house.toml
transition: pan-up
---

<!-- slide: transition=slide-left -->
# One

```rust {sise=20}
a
```
";
        let mut client = Client::start(json!({}));
        client.open(&uri, deck);
        let diagnostics = client.diagnostics();
        assert_eq!(codes(&diagnostics), ["fence-option"], "{diagnostics:#}");

        // Hover on the transition value explains the pan.
        let hover = client.request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": uri }, "position": { "line": 5, "character": 26 } }),
        );
        let text = hover["contents"]["value"].as_str().unwrap();
        assert!(text.starts_with("A pan: the content slides left"), "{text}");
        assert_eq!(hover["contents"]["kind"], "markdown");
        assert_eq!(hover["range"]["start"]["character"], 23);
        // Nothing on ordinary text.
        let none = client.request(
            "textDocument/hover",
            json!({ "textDocument": { "uri": uri }, "position": { "line": 6, "character": 3 } }),
        );
        assert!(none.is_null());

        // The theme path is a link.
        let links = client.request(
            "textDocument/documentLink",
            json!({ "textDocument": { "uri": uri } }),
        );
        let targets: Vec<&str> = links
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["target"].as_str().unwrap())
            .collect();
        assert!(
            targets.iter().any(|t| t.ends_with("themes/house.toml")),
            "{targets:?}"
        );

        // A theme that isn't there is reported, with where themes are looked for.
        let missing = deck.replace("themes/house.toml", "themes/gone.toml");
        client.notify(
            "textDocument/didChange",
            json!({ "textDocument": { "uri": uri, "version": 2 }, "contentChanges": [{ "text": missing }] }),
        );
        let diagnostics = client.diagnostics();
        let theme = diagnostics
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["code"] == "missing-file")
            .expect("missing theme reported");
        assert!(
            theme["message"]
                .as_str()
                .unwrap()
                .starts_with("theme not found: themes/gone.toml")
        );
    }

    #[test]
    fn zoom_is_checked_and_completed() {
        let dir = deck_dir("zoom");
        let uri = uri_of(&dir.join("talk.md"));
        let deck = "\
# Pipeline

```mermaid {all|Layout|Nope zoom}
graph LR
  a[Parse input] --> b[Layout]
```

<!-- zoom[1]: 140%,20%,2x -->
";
        let mut client = Client::start(json!({}));
        client.open(&uri, deck);
        let diagnostics = client.diagnostics();
        let mut found = codes(&diagnostics);
        found.sort_unstable();
        assert_eq!(found, ["zoom-dropped", "zoom-label"], "{diagnostics:#}");
        // The label warning points at `Nope`, not the whole fence.
        let label = diagnostics
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["code"] == "zoom-label")
            .unwrap();
        let line = deck.lines().nth(2).unwrap();
        let start = line.find("Nope").unwrap();
        assert_eq!(label["range"]["start"]["line"], 2);
        assert_eq!(label["range"]["start"]["character"], start);
        assert_eq!(label["range"]["end"]["character"], start + "Nope".len());

        // Inside a stage, completion offers the diagram's node labels.
        let items = client.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 2, "character": line.find('|').unwrap() + 1 },
            }),
        );
        let labels: Vec<&str> = items
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["label"].as_str().unwrap())
            .collect();
        for wanted in ["Parse input", "Layout", "zoom"] {
            assert!(labels.contains(&wanted), "{wanted} in {labels:?}");
        }
    }

    #[test]
    fn diagnostics_on_open_and_change() {
        let dir = deck_dir("diag");
        let uri = uri_of(&dir.join("talk.md"));
        let client = Client::start(json!({}));
        client.open(&uri, DECK);
        let d = client.diagnostics();
        assert_eq!(codes(&d), vec!["unknown-directive", "missing-file"]);
        // The misspelt name, in UTF-16 columns.
        assert_eq!(
            d[0]["range"],
            json!({"start": {"line": 11, "character": 5}, "end": {"line": 11, "character": 11}})
        );

        client.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": 2 },
                "contentChanges": [{ "text": "# Clean\n" }]
            }),
        );
        assert_eq!(client.diagnostics(), json!([]));
    }

    #[test]
    fn outline_folds_and_links() {
        let dir = deck_dir("outline");
        let uri = uri_of(&dir.join("talk.md"));
        let mut client = Client::start(json!({}));
        client.open(&uri, DECK);
        let doc = json!({ "textDocument": { "uri": uri } });

        let symbols = client.request("textDocument/documentSymbol", doc.clone());
        let names: Vec<_> = symbols
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["name"].as_str().unwrap(), s["detail"].as_str().unwrap()))
            .collect();
        assert_eq!(names, vec![("Intro", "slide 1"), ("Two", "slide 2")]);
        assert_eq!(symbols[1]["selectionRange"]["start"]["line"], 12);

        let folds = client.request("textDocument/foldingRange", doc.clone());
        let spans: Vec<_> = folds
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["startLine"].as_u64().unwrap(),
                    f["endLine"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(spans, vec![(0, 2), (4, 7), (11, 16)]);

        let links = client.request("textDocument/documentLink", doc);
        let targets: Vec<_> = links
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["target"].as_str().unwrap().to_string())
            .collect();
        let expected = std::path::absolute(dir.join("img/there.png")).unwrap();
        assert_eq!(targets, vec![uri_of(&expected)]);
    }

    #[test]
    fn code_actions_filtered_by_kind_and_quick_fixes() {
        let dir = deck_dir("actions");
        let uri = uri_of(&dir.join("talk.md"));
        let mut client = Client::start(json!({}));
        client.open(&uri, DECK);
        let at = |line: u32, only: Option<Vec<&str>>| {
            let mut context = json!({ "diagnostics": [] });
            if let Some(only) = only {
                context["only"] = json!(only);
            }
            json!({
                "textDocument": { "uri": uri },
                "range": { "start": { "line": line, "character": 0 },
                           "end": { "line": line, "character": 0 } },
                "context": context,
            })
        };

        // Bound to a key: exactly the one action.
        let actions = client.request(
            "textDocument/codeAction",
            at(16, Some(vec!["refactor.preso.layout.twoColumns"])),
        );
        let actions = actions.as_array().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0]["title"], "Layout: two columns, split here");
        let edits = &actions[0]["edit"]["changes"][&uri];
        assert_eq!(edits.as_array().unwrap().len(), 1);
        let text = edits[0]["newText"].as_str().unwrap();
        assert!(text.ends_with("left\n\n***\n\n"), "{text:?}");
        assert_eq!(edits[0]["range"]["end"]["line"], 16);

        // Unfiltered on the misspelt directive: the quick fix leads.
        let actions = client.request("textDocument/codeAction", at(11, None));
        assert_eq!(actions[0]["kind"], "quickfix");
        assert_eq!(actions[0]["title"], "Change to `layout`");
        let kinds: Vec<_> = actions
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["kind"].as_str().unwrap())
            .collect();
        assert!(kinds.contains(&"refactor.preso.slide.moveUp"));
        assert!(!kinds.contains(&"refactor.preso.slide.moveDown"));

        // A `refactor` filter takes every structural action and no fixes.
        let actions = client.request("textDocument/codeAction", at(11, Some(vec!["refactor"])));
        assert!(
            actions
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["kind"].as_str().unwrap().starts_with("refactor.preso."))
        );
    }

    #[test]
    fn completions_for_directives_values_and_paths() {
        let dir = deck_dir("complete");
        let uri = uri_of(&dir.join("talk.md"));
        let caps = json!({ "textDocument": { "completion": {
            "completionItem": { "snippetSupport": true } } } });
        let mut client = Client::start(caps);
        client.open(&uri, "<!-- \n<!-- slide: kind=\n![x](img/\n");
        let mut at = |line: u32, character: u32| {
            client.request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                }),
            )
        };

        let directives = at(0, 5);
        let first = &directives[0];
        assert_eq!(first["label"], "layout: TwoColumn");
        assert_eq!(first["filterText"], "<!-- layout: TwoColumn -->");
        let ratio = &directives[1];
        assert_eq!(ratio["insertTextFormat"], 2); // snippet
        assert_eq!(
            ratio["textEdit"]["newText"],
            "<!-- layout: TwoColumn ${1:2:1} -->"
        );

        let labels = |v: &Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|i| i["label"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(labels(&at(1, 17)), vec!["title", "section"]);
        // Only formats preso renders.
        assert_eq!(labels(&at(2, 9)), vec!["there.png"]);
    }

    #[test]
    fn recent_unused_media_is_offered_for_insertion() {
        let dir = deck_dir("media");
        let at = |secs: u64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        for (name, secs) in [
            ("img/there.png", 500),
            ("img/old.png", 1_000),
            ("img/newest.png", 3_000),
            ("img/used.png", 4_000),
            ("img/middle.jpg", 2_000),
            ("img/has space.png", 5_000),
            ("clips/demo.mp4", 2_500),
        ] {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let file = std::fs::File::create(&path).unwrap();
            file.set_modified(at(secs)).unwrap();
        }
        // The deck: two chapters, the second already using `middle.jpg`.
        std::fs::create_dir_all(dir.join("ch")).unwrap();
        std::fs::write(
            dir.join("talk.md"),
            "<!-- include: ch/one.md -->\n<!-- include: ch/two.md -->\n",
        )
        .unwrap();
        std::fs::write(dir.join("ch/one.md"), "").unwrap();
        std::fs::write(dir.join("ch/two.md"), "![](img/middle.jpg)\n").unwrap();
        let uri = uri_of(&dir.join("ch/one.md"));
        let mut client = Client::start(json!({}));
        client.open(&uri, "# Slide\n\n![](img/used.png)\n\n- point\n");
        let actions = client.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri },
                "range": { "start": { "line": 4, "character": 0 },
                           "end": { "line": 4, "character": 0 } },
                "context": { "diagnostics": [] },
            }),
        );
        let titles: Vec<&str> = actions
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["title"].as_str().unwrap())
            .collect();
        let inserts: Vec<&str> = titles
            .iter()
            .copied()
            .filter(|t| t.starts_with("Insert") || t.starts_with("Background"))
            .collect();
        // Newest first. Left out: the image this chapter uses, the one its
        // sibling uses, and the name markdown can't hold. Paths are relative
        // to the top-level deck.
        assert_eq!(
            inserts,
            vec![
                "Insert image: img/newest.png",
                "Insert image: img/old.png",
                "Insert image: img/there.png",
                "Background image: img/newest.png",
                "Insert video: clips/demo.mp4",
            ]
        );
        // Grouped: layout, then inserts, then slide operations.
        let group = |t: &str| {
            if t.starts_with("Layout") {
                0
            } else if t.starts_with("Slide") {
                2
            } else {
                1
            }
        };
        assert!(
            titles.windows(2).all(|w| group(w[0]) <= group(w[1])),
            "{titles:?}"
        );

        let image = &actions
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["title"] == "Insert image: img/newest.png")
            .unwrap()["edit"]["changes"][&uri][0];
        assert_eq!(image["newText"], "\n![newest](img/newest.png)\n");
        assert_eq!(image["range"]["start"]["line"], 5);
    }

    #[test]
    fn styles_follow_the_cursor() {
        let mut client = Client::start(json!({}));
        let uri = "file:///nowhere/talk.md";
        client.open(
            uri,
            "# Slide\n\n![Logo](logo.png)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nwords\n",
        );
        let mut titles_at = |line: u32, character: u32| -> Vec<String> {
            let actions = client.request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": { "start": { "line": line, "character": character },
                               "end": { "line": line, "character": character } },
                    "context": { "diagnostics": [] },
                }),
            );
            actions
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a["title"].as_str().unwrap().to_string())
                .collect()
        };
        let on_image = titles_at(2, 4);
        assert_eq!(on_image[0], "Image: width 25%");
        assert!(!on_image.iter().any(|t| t.starts_with("Table")));
        let on_table = titles_at(6, 6); // in column 2
        assert_eq!(on_table[0], "Table: text size 28");
        assert!(
            on_table.contains(&"Table: centre column 2 (b)".to_string()),
            "{on_table:?}"
        );
        let on_words = titles_at(8, 0);
        assert!(on_words[0].starts_with("Layout"), "{on_words:?}");
    }

    #[test]
    fn utf8_positions_when_the_client_offers_them() {
        let caps = json!({ "general": { "positionEncodings": ["utf-8", "utf-16"] } });
        let client = Client::start(caps);
        client.open("file:///nowhere/talk.md", "<!-- slide: é bogus -->\n");
        let d = client.diagnostics();
        // `bogus` starts at byte 15, UTF-16 unit 14: the `é` is two bytes.
        assert_eq!(d[1]["range"]["start"]["character"], 15);
    }

    #[test]
    fn requests_for_unopened_documents_return_null() {
        let mut client = Client::start(json!({}));
        let r = client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": "file:///not/open.md" } }),
        );
        assert_eq!(r, Value::Null);
    }
}
