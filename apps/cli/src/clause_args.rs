//! Translate clause boundaries into the existing typed filter groups before command parsing.
//! Predicate names and value validation come from the same clap definition as ordinary filters.
use std::collections::HashSet;
use std::ffi::OsString;

use clap::{CommandFactory, Parser, error::ErrorKind};
use weave_player_lib::core::FilterSpec;

use crate::{FilterArgs, parse_filter_group};

#[derive(Parser)]
#[command(name = "filter clause", disable_help_flag = true)]
struct Clause {
    #[command(flatten)]
    filter: FilterArgs,
}

struct Draft {
    base: FilterSpec,
    args: Vec<OsString>,
}
impl Draft {
    fn finish(self, output: &mut Vec<OsString>) -> Result<(), clap::Error> {
        let clause = Clause::try_parse_from(self.args)?;
        let filter = self.base.merged(clause.filter.into());
        filter
            .validate()
            .map_err(|error| clap::Error::raw(ErrorKind::ValueValidation, error.to_string()))?;
        let json = serde_json::to_string(&filter)
            .map_err(|error| clap::Error::raw(ErrorKind::ValueValidation, error.to_string()))?;
        output.push(format!("--filter-group={json}").into());
        Ok(())
    }
}

pub fn expand(args: Vec<OsString>) -> Result<Vec<OsString>, clap::Error> {
    let args = expand_tag_blocks(args)?;
    let command = Clause::command();
    let predicates: HashSet<&str> = command
        .get_arguments()
        .filter(|arg| {
            !matches!(
                arg.get_id().as_str(),
                "groups" | "group_join" | "sort" | "desc" | "limit"
            )
        })
        .filter_map(|arg| arg.get_long())
        .collect();
    let mut input = args.into_iter().peekable();
    let mut output = Vec::new();
    let mut draft: Option<Draft> = None;
    while let Some(arg) = input.next() {
        if arg == "--" {
            if let Some(draft) = draft.take() {
                draft.finish(&mut output)?;
            }
            output.push(arg);
            output.extend(input);
            return Ok(output);
        }
        let flag_owned = arg.to_string_lossy().into_owned();
        let flag = flag_owned.as_str();
        let (name, inline) = flag
            .split_once('=')
            .map_or((flag, None), |(name, value)| (name, Some(value)));
        if name == "--filter-group" {
            if let Some(previous) = draft.take() {
                previous.finish(&mut output)?;
            }
            let json = if let Some(value) = inline {
                Some(OsString::from(value))
            } else if input
                .peek()
                .is_some_and(|next| !next.to_string_lossy().starts_with('-'))
            {
                input.next()
            } else {
                None
            };
            let base = match json {
                Some(value) => {
                    let text = value.to_str().ok_or_else(|| {
                        clap::Error::raw(ErrorKind::InvalidUtf8, "Filter JSON must be UTF-8")
                    })?;
                    parse_filter_group(text)
                        .map_err(|error| clap::Error::raw(ErrorKind::ValueValidation, error))?
                }
                None => FilterSpec::default(),
            };
            draft = Some(Draft {
                base,
                args: vec!["filter clause".into()],
            });
        } else if let Some(current) = &mut draft
            && name
                .strip_prefix("--")
                .is_some_and(|name| predicates.contains(name))
        {
            current.args.push(arg);
            if inline.is_none()
                && let Some(value) = input.next()
            {
                current.args.push(value);
            }
        } else {
            output.push(arg);
            // Keep values of outer options together, so literal values cannot start a clause.
            if inline.is_none()
                && matches!(
                    name,
                    "--db"
                        | "--group-join"
                        | "--sort"
                        | "--limit"
                        | "--page"
                        | "--per-page"
                        | "--description"
                        | "--period"
                        | "--since-ms"
                        | "--until-ms"
                        | "--top-n"
                )
                && let Some(value) = input.next()
            {
                output.push(value);
            }
        }
    }
    if let Some(draft) = draft {
        draft.finish(&mut output)?;
    }
    Ok(output)
}

/// Flat OR alternatives, separated into AND blocks. Non-tag predicates stay global.
fn expand_tag_blocks(args: Vec<OsString>) -> Result<Vec<OsString>, clap::Error> {
    let mut output = Vec::new();
    let mut draft: Option<Vec<OsString>> = None;
    let mut block = Vec::new();
    let mut blocks = Vec::new();
    let mut input = args.into_iter();
    fn finish(
        args: Vec<OsString>,
        block: &mut Vec<weave_player_lib::core::TagFilter>,
    ) -> Result<(), clap::Error> {
        let parsed = Clause::try_parse_from(args)?;
        block.push(parsed.filter.tags.spec().tag_filter());
        Ok(())
    }
    while let Some(arg) = input.next() {
        let text = arg.to_string_lossy();
        let name = text.split('=').next().unwrap_or("");
        if name == "--" {
            output.push(arg);
            output.extend(input);
            break;
        } else if matches!(name, "--or-group" | "--and-group") {
            if let Some(previous) = draft.take() {
                finish(previous, &mut block)?;
            }
            if name == "--and-group" && !block.is_empty() {
                blocks.push(std::mem::take(&mut block));
            }
            draft = Some(vec!["tag group".into()]);
        } else if let Some(current) = &mut draft
            && matches!(name, "--tag-all" | "--tag-any" | "--tag-exclude")
        {
            let inline = text.contains('=');
            current.push(arg);
            if !inline && let Some(value) = input.next() {
                current.push(value);
            }
        } else {
            // Preserve values verbatim, including values that resemble group delimiters.
            let takes_value = name.starts_with("--")
                && !text.contains('=')
                && !matches!(
                    name,
                    "--json" | "--desc" | "--help" | "--version" | "--yes" | "--shuffle"
                );
            output.push(arg);
            if takes_value
                && input
                    .as_slice()
                    .first()
                    .is_some_and(|v| !v.to_string_lossy().starts_with("--"))
            {
                output.push(input.next().unwrap());
            }
        }
    }
    if let Some(last) = draft {
        finish(last, &mut block)?;
    }
    if !block.is_empty() {
        blocks.push(block);
    }
    // Insert before an end-of-options marker, if any.
    let index = output
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(output.len());
    for block in blocks.into_iter().rev() {
        output.insert(
            index,
            format!("--tag-block={}", serde_json::to_string(&block).unwrap()).into(),
        );
    }
    Ok(output)
}
