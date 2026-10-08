use std::ops::Range;
use std::path::{Path, PathBuf};

use hcl_edit::structure::{Attribute, Block, Body, Structure};
use hcl_edit::Span;

use crate::diag::Diag;

/// One parsed HCL file, kept with its text so any span can be turned into a line and column.
pub struct Source {
    pub path: PathBuf,
    pub text: String,
    pub body: Body,
}

impl Source {
    pub fn read(path: &Path) -> Result<Source, Diag> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Diag::file(path, format!("cannot read: {e}")))?;
        Source::parse(path, text)
    }

    pub fn parse(path: &Path, text: String) -> Result<Source, Diag> {
        match hcl_edit::parser::parse_body(&text) {
            Ok(body) => Ok(Source { path: path.to_path_buf(), text, body }),
            Err(e) => {
                let at = e.location();
                Err(Diag::new(path, at.line() as u32, at.column() as u32, syntax_message(e.message())))
            }
        }
    }

    /// The directory holding the file, what `path.file` evaluates to.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    pub fn diag(&self, span: Option<Range<usize>>, message: impl Into<String>) -> Diag {
        let (line, column) = match span {
            Some(r) => self.line_column(r.start),
            None => (0, 0),
        };
        Diag::new(&self.path, line, column, message)
    }

    pub fn diag_at(&self, item: &dyn Span, message: impl Into<String>) -> Diag {
        self.diag(item.span(), message)
    }

    fn line_column(&self, offset: usize) -> (u32, u32) {
        let before = &self.text[..offset.min(self.text.len())];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let column = before[line_start..].chars().count() + 1;
        (line as u32, column as u32)
    }

    /// The top level blocks. A top level attribute is refused, since no file kind has one.
    pub fn top_blocks(&self) -> Result<Vec<&Block>, Diag> {
        top_blocks(self, &self.body)
    }
}

fn top_blocks<'a>(src: &Source, body: &'a Body) -> Result<Vec<&'a Block>, Diag> {
    let mut blocks = Vec::new();
    for s in body.iter() {
        match s {
            Structure::Block(b) => blocks.push(b),
            Structure::Attribute(a) => {
                return Err(src.diag_at(a, format!("unexpected attribute `{}` at the top level", a.key.as_str())))
            }
        }
    }
    Ok(blocks)
}

/// The attributes and blocks of a block body, with every key checked against what the block
/// allows, so a typo is an error rather than silently ignored.
pub struct Fields<'a> {
    pub attrs: Vec<&'a Attribute>,
    pub blocks: Vec<&'a Block>,
}

impl<'a> Fields<'a> {
    pub fn of(src: &Source, block: &'a Block, attrs: &[&str], blocks: &[&str]) -> Result<Fields<'a>, Diag> {
        Fields::of_body(src, &block.body, block.ident.as_str(), attrs, blocks)
    }

    /// The same for a whole body, such as the top level of a file. kind names it in errors.
    pub fn of_body(src: &Source, body: &'a Body, kind: &str, attrs: &[&str], blocks: &[&str]) -> Result<Fields<'a>, Diag> {
        let mut out = Fields { attrs: Vec::new(), blocks: Vec::new() };
        for s in body.iter() {
            match s {
                Structure::Attribute(a) => {
                    let key = a.key.as_str();
                    if !attrs.contains(&key) {
                        return Err(src.diag_at(a, format!("unknown attribute `{key}` in {kind}{}", allowed(attrs))));
                    }
                    if out.attrs.iter().any(|x| x.key.as_str() == key) {
                        return Err(src.diag_at(a, format!("`{key}` is set twice")));
                    }
                    out.attrs.push(a);
                }
                Structure::Block(b) => {
                    let ident = b.ident.as_str();
                    if !blocks.contains(&ident) {
                        return Err(src.diag_at(b, format!("unknown block `{ident}` in {kind}")));
                    }
                    out.blocks.push(b);
                }
            }
        }
        Ok(out)
    }

    pub fn attr(&self, key: &str) -> Option<&'a Attribute> {
        self.attrs.iter().copied().find(|a| a.key.as_str() == key)
    }

    pub fn blocks(&self, ident: &'a str) -> impl Iterator<Item = &'a Block> + '_ {
        self.blocks.iter().copied().filter(move |b| b.ident.as_str() == ident)
    }
}

fn allowed(attrs: &[&str]) -> String {
    if attrs.is_empty() {
        String::new()
    } else {
        format!(", expected one of: {}", attrs.join(", "))
    }
}

/// The one label a block must carry, such as `param "target"`.
pub fn one_label<'a>(src: &Source, block: &'a Block) -> Result<&'a str, Diag> {
    match block.labels.as_slice() {
        [label] => Ok(label.as_str()),
        _ => Err(src.diag_at(block, format!("`{}` takes exactly one label", block.ident.as_str()))),
    }
}

pub fn no_labels(src: &Source, block: &Block) -> Result<(), Diag> {
    if block.labels.is_empty() {
        Ok(())
    } else {
        Err(src.diag_at(block, format!("`{}` takes no label", block.ident.as_str())))
    }
}

/// The parser's messages name what it expected. A leading capital reads oddly after a location.
fn syntax_message(message: &str) -> String {
    let m = message.trim();
    if m.is_empty() {
        "syntax error".to_string()
    } else {
        format!("syntax error: {m}")
    }
}

/// A place in a source file, kept so a later error can point back at it.
#[derive(Debug, Clone)]
pub struct Loc {
    file: PathBuf,
    line: u32,
    column: u32,
}

impl Loc {
    pub fn of(src: &Source, item: &dyn Span) -> Loc {
        let d = src.diag_at(item, "");
        Loc { file: d.file, line: d.line, column: d.column }
    }

    pub fn diag(&self, message: impl Into<String>) -> Diag {
        Diag::new(&self.file, self.line, self.column, message)
    }

    pub fn shown(&self) -> String {
        format!("{}:{}:{}", self.file.display(), self.line, self.column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_error_is_located() {
        let err = Source::parse(Path::new("rant.hcl"), "workspace {\n  logs = \n}\n".into()).err().unwrap();
        assert!(err.line >= 2, "{err}");
        assert!(err.message.starts_with("syntax error"), "{}", err.message);
    }

    #[test]
    fn unknown_attribute_is_located() {
        let src = Source::parse(Path::new("rant.hcl"), "workspace {\n  lgos = \"x\"\n}\n".into()).unwrap();
        let blocks = src.top_blocks().unwrap();
        let err = Fields::of(&src, blocks[0], &["logs", "ignore"], &[]).err().unwrap();
        assert_eq!((err.line, err.column), (2, 3));
        assert!(err.message.contains("lgos"));
    }
}
