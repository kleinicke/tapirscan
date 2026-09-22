//! Relocate Rust modules by token spans, preserving literals, comments and layout.
#![forbid(unsafe_code)]
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read},
    ops::Range,
};

#[derive(Deserialize)]
struct Request {
    namespace: String,
    features: BTreeSet<String>,
    aliases: BTreeMap<String, String>,
    files: BTreeMap<String, String>,
}
#[derive(Serialize)]
struct Response {
    flags: BTreeMap<String, bool>,
    files: BTreeMap<String, String>,
}
struct Edit {
    range: Range<usize>,
    replacement: String,
}
struct Rewriter<'a> {
    request: &'a Request,
    flags: BTreeMap<String, bool>,
}
fn ident(token: Option<&TokenTree>, name: &str) -> bool {
    matches!(token, Some(TokenTree::Ident(value)) if value == name)
}
fn punct(token: Option<&TokenTree>, ch: char) -> bool {
    matches!(token, Some(TokenTree::Punct(value)) if value.as_char() == ch)
}
fn export_attribute(group: &proc_macro2::Group) -> bool {
    let tokens: Vec<_> = group.stream().into_iter().collect();
    if tokens.len() == 1 && ident(tokens.first(), "no_mangle") {
        return true;
    }
    if tokens.len() == 2 && ident(tokens.first(), "unsafe") {
        if let TokenTree::Group(inner) = &tokens[1] {
            return export_attribute(inner);
        }
    }
    false
}
impl Rewriter<'_> {
    fn visit(
        &mut self,
        stream: TokenStream,
        configuration: bool,
        attribute: bool,
        edits: &mut Vec<Edit>,
    ) -> Result<(), String> {
        let tokens: Vec<_> = stream.into_iter().collect();
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            if punct(Some(token), '#') {
                if let Some(TokenTree::Group(group)) = tokens.get(index + 1) {
                    if group.delimiter() == Delimiter::Bracket && export_attribute(group) {
                        edits.push(Edit {
                            range: token.span().byte_range().start..group.span().byte_range().end,
                            replacement: String::new(),
                        });
                        index += 2;
                        continue;
                    }
                }
            }
            if configuration && ident(Some(token), "feature") && punct(tokens.get(index + 1), '=') {
                let literal = tokens.get(index + 2).ok_or("missing feature literal")?;
                let TokenTree::Literal(value) = literal else {
                    return Err("feature must have a string literal".into());
                };
                let feature = syn::parse_str::<syn::LitStr>(&value.to_string())
                    .map_err(|e| e.to_string())?
                    .value();
                let name = format!(
                    "tapirscan_{}_{}",
                    self.request.namespace,
                    feature.replace('-', "_")
                );
                self.flags
                    .insert(name.clone(), self.request.features.contains(&feature));
                edits.push(Edit {
                    range: token.span().byte_range().start..literal.span().byte_range().end,
                    replacement: name,
                });
                index += 3;
                continue;
            }
            if let TokenTree::Ident(name) = token {
                // Only a path root is relocated, not a same-named nested module.
                let root = index < 2
                    || !(punct(tokens.get(index - 1), ':') && punct(tokens.get(index - 2), ':'));
                if root && punct(tokens.get(index + 1), ':') && punct(tokens.get(index + 2), ':') {
                    let name = name.to_string();
                    let replacement = if name == "crate" {
                        Some(format!("crate::engine::{}", self.request.namespace))
                    } else {
                        self.request.aliases.get(&name).map(|target| {
                            if target.starts_with("crate::") {
                                target.clone()
                            } else {
                                format!("crate::engine::{target}")
                            }
                        })
                    };
                    if let Some(replacement) = replacement {
                        edits.push(Edit {
                            range: token.span().byte_range(),
                            replacement,
                        });
                    }
                }
            }
            if let TokenTree::Group(group) = token {
                let previous = index.checked_sub(1).and_then(|i| tokens.get(i));
                let macro_name = index.checked_sub(2).and_then(|i| tokens.get(i));
                if attribute && ident(previous, "cfg_attr") {
                    let inner: Vec<_> = group.stream().into_iter().collect();
                    let comma = inner
                        .iter()
                        .position(|t| punct(Some(t), ','))
                        .ok_or("cfg_attr needs an attribute")?;
                    self.visit(inner[..comma].iter().cloned().collect(), true, false, edits)?;
                    self.visit(
                        inner[comma + 1..].iter().cloned().collect(),
                        false,
                        true,
                        edits,
                    )?;
                } else {
                    let cfg = configuration
                        || (attribute && ident(previous, "cfg"))
                        || (punct(previous, '!') && ident(macro_name, "cfg"));
                    let attr = group.delimiter() == Delimiter::Bracket && punct(previous, '#');
                    self.visit(group.stream(), cfg, attr, edits)?;
                }
            }
            index += 1;
        }
        Ok(())
    }
    fn rewrite(&mut self, source: &str) -> Result<String, String> {
        let tokens = source.parse::<TokenStream>().map_err(|e| e.to_string())?;
        let mut edits = Vec::new();
        self.visit(tokens, false, false, &mut edits)?;
        edits.sort_by_key(|edit| edit.range.start);
        let mut result = String::with_capacity(source.len());
        let mut cursor = 0;
        for mut edit in edits {
            if edit.replacement.is_empty() {
                // Drop the whole attribute line when it contains no other content.
                // Leaving a blank line would detach adjacent documentation/attributes.
                let start = source[..edit.range.start].rfind('\n').map_or(0, |i| i + 1);
                let end = source[edit.range.end..]
                    .find('\n')
                    .map(|i| edit.range.end + i);
                if let Some(end) = end {
                    if source[start..edit.range.start].trim().is_empty()
                        && source[edit.range.end..end].trim().is_empty()
                    {
                        edit.range = start..end + 1;
                    }
                }
            }

            if edit.range.start < cursor {
                return Err("overlapping source rewrites".into());
            }
            result.push_str(
                source
                    .get(cursor..edit.range.start)
                    .ok_or("invalid source span")?,
            );
            result.push_str(&edit.replacement);
            cursor = edit.range.end;
        }
        result.push_str(source.get(cursor..).ok_or("invalid final source span")?);
        Ok(result)
    }
}
fn transform(request: &Request) -> Result<Response, String> {
    let mut rewriter = Rewriter {
        request,
        flags: BTreeMap::new(),
    };
    let mut files = BTreeMap::new();
    for (path, source) in &request.files {
        files.insert(
            path.clone(),
            rewriter
                .rewrite(source)
                .map_err(|e| format!("{path}: {e}"))?,
        );
    }
    Ok(Response {
        files,
        flags: rewriter.flags,
    })
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let request = serde_json::from_str(&input)?;
    let result = transform(&request).map_err(io::Error::other)?;
    serde_json::to_writer(io::stdout(), &result)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(source: &str) -> Response {
        transform(&Request {
            namespace: "core_low".into(),
            features: BTreeSet::from(["mode-low".into()]),
            aliases: BTreeMap::from([("decoder".into(), "shared".into())]),
            files: BTreeMap::from([("lib.rs".into(), source.into())]),
        })
        .unwrap()
    }
    #[test]
    fn preserves_literals_comments_and_unicode_spans() {
        let input = "// café crate::x feature=\"bad\"\n/* outer /* crate::nested */ decoder::thing */\nconst DOC: &str = r#\"crate::x #[no_mangle] feature=\"no\"\"#;\nconst CH: char = 'é';\nfn f<'a>() { crate::scan(); decoder::read(); nested::decoder::stay(); }\n";
        let output = run(input);
        assert_eq!(
            output.files["lib.rs"],
            input.replace(
                "{ crate::scan(); decoder::read();",
                "{ crate::engine::core_low::scan(); crate::engine::shared::read();"
            )
        );
        assert!(output.flags.is_empty());
    }
    #[test]
    fn rewrites_cfg_attributes_and_macros_but_not_ordinary_assignments() {
        let input = r#"#[cfg(all(feature="mode-low", target_arch="wasm32"))]
#[cfg_attr(not(feature = "mode-high"), allow(dead_code))]
fn f() { let feature = "mode-low"; assert!(cfg!(feature = "mode-high")); crate::scan(); }
"#;
        let output = run(input);
        let text = &output.files["lib.rs"];
        assert!(text.contains("all(tapirscan_core_low_mode_low, target_arch=\"wasm32\")"));
        assert!(text.contains("not(tapirscan_core_low_mode_high)"));
        assert!(text.contains("cfg!(tapirscan_core_low_mode_high)"));
        assert!(text.contains("let feature = \"mode-low\""));
        assert_eq!(
            output.flags,
            BTreeMap::from([
                ("tapirscan_core_low_mode_low".into(), true),
                ("tapirscan_core_low_mode_high".into(), false)
            ])
        );
    }
    #[test]
    fn removes_only_real_export_attributes_and_keeps_use_trees() {
        let output = run("#[no_mangle] pub extern \"C\" fn f() {}\n#[unsafe(no_mangle)] fn g() {}\nuse {crate::a, decoder::{b,c}};");
        assert_eq!(output.files["lib.rs"], " pub extern \"C\" fn f() {}\n fn g() {}\nuse {crate::engine::core_low::a, crate::engine::shared::{b,c}};");
    }
    #[test]
    fn removing_exports_preserves_adjacent_documentation() {
        let input =
            "/// Function.\n    #[no_mangle]\nfn f() {}\n#[no_mangle] // keep comment\nfn g() {}";
        assert_eq!(
            run(input).files["lib.rs"],
            "/// Function.\nfn f() {}\n // keep comment\nfn g() {}"
        );
    }
    #[test]
    fn scopes_feature_rewrites_to_predicates() {
        let input = r#"#[cfg_attr(feature="mode-low", custom(feature="literal"), cfg(feature="mode-high"))]
fn f() { cfg(feature="ordinary"); }
"#;
        let output = run(input);
        let text = &output.files["lib.rs"];
        assert!(text.contains("cfg_attr(tapirscan_core_low_mode_low, custom(feature=\"literal\"), cfg(tapirscan_core_low_mode_high))"));
        assert!(text.contains("cfg(feature=\"ordinary\")"));
        assert_eq!(output.flags.len(), 2);
    }
    #[test]
    fn retains_conditional_exports_under_the_relocated_feature() {
        let output = run("#[cfg_attr(feature=\"ffi\", no_mangle)] fn f() {}");
        assert_eq!(
            output.files["lib.rs"],
            "#[cfg_attr(tapirscan_core_low_ffi, no_mangle)] fn f() {}"
        );
        assert!(!output.flags["tapirscan_core_low_ffi"]);
    }
    #[test]
    fn reports_invalid_input_with_filename() {
        let request = Request {
            namespace: "x".into(),
            features: BTreeSet::new(),
            aliases: BTreeMap::new(),
            files: BTreeMap::from([("broken.rs".into(), "fn {".into())]),
        };
        assert!(transform(&request).err().unwrap().contains("broken.rs"));
    }
}
