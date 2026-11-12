// SPDX-License-Identifier: GPL-3.0-or-later

//! `destinations.toml`: reading the profiles, adding, replacing and removing one. The file only holds
//! names of secrets.
//!
//! Every change goes through `toml_edit`, so that the comments and the order of the file stay as the
//! user left them, and the whole result is parsed and validated again before it replaces the file in
//! one step: an invalid result changes nothing.

use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::destination::{DESTINATIONS_FILE, Destination, DestinationFile};
use crate::error::UploadError;

/// Path of the destinations file inside the configuration folder `dir`.
#[must_use]
pub fn destinations_path(dir: &Path) -> PathBuf {
    dir.join(DESTINATIONS_FILE)
}

fn io(path: &Path, source: std::io::Error) -> UploadError {
    UploadError::Io {
        path: path.to_owned(),
        source,
    }
}

fn read(path: &Path) -> Result<Option<String>, UploadError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

/// The destinations of the configuration folder `dir`. A missing file is no destination.
///
/// # Errors
/// [`UploadError::Io`] if the file cannot be read, [`UploadError::Config`] if it is not valid.
pub fn load_destinations(dir: &Path) -> Result<Vec<Destination>, UploadError> {
    let path = destinations_path(dir);
    match read(&path)? {
        None => Ok(Vec::new()),
        Some(text) => {
            DestinationFile::parse(&text, &path.display().to_string()).map(|file| file.destination)
        }
    }
}

/// The destination `id`.
///
/// # Errors
/// [`UploadError::Config`] (`unknown_destination`) when there is none.
pub fn find<'a>(destinations: &'a [Destination], id: &str) -> Result<&'a Destination, UploadError> {
    destinations
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| UploadError::Config {
            reason: "unknown_destination",
            detail: format!("no destination named {id:?}"),
        })
}

/// Adds `destination` at the end of the file, keeping what is there (comments included). The result
/// is parsed again before it replaces the file, and the file is replaced in one step.
///
/// # Errors
/// [`UploadError::Config`] if the `id` is taken or the result is not valid, [`UploadError::Io`] if the
/// file cannot be written.
pub fn append_destination(dir: &Path, destination: &Destination) -> Result<(), UploadError> {
    let path = destinations_path(dir);
    let mut text = read(&path)?.unwrap_or_default();
    let shown = path.display().to_string();
    if DestinationFile::parse(&text, &shown)?
        .destination
        .iter()
        .any(|d| d.id == destination.id)
    {
        return Err(UploadError::config(format!(
            "the destination {:?} already exists",
            destination.id
        )));
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&destination.to_toml()?);
    DestinationFile::parse(&text, &shown)?;
    write(&path, &text)
}

fn write(path: &Path, text: &str) -> Result<(), UploadError> {
    kropa_core::write_atomic(path, text).map_err(|e| io(path, e))
}

fn parse_document(text: &str, shown: &str) -> Result<DocumentMut, UploadError> {
    text.parse::<DocumentMut>()
        .map_err(|e| UploadError::config(format!("{shown}: {}", e.message())))
}

fn unknown(id: &str) -> UploadError {
    UploadError::Config {
        reason: "unknown_destination",
        detail: format!("no destination named {id:?}"),
    }
}

/// The position of the table `id` in the `[[destination]]` array.
fn index_of(doc: &DocumentMut, id: &str) -> Option<usize> {
    doc.get("destination")?
        .as_array_of_tables()?
        .iter()
        .position(|t| t.get("id").and_then(Item::as_str) == Some(id))
}

fn destinations_mut(doc: &mut DocumentMut) -> Option<&mut ArrayOfTables> {
    doc.get_mut("destination")?.as_array_of_tables_mut()
}

/// A table that comes from another document must not bring its place in the file: without one it is
/// written right after the table before it, under the right `[[destination]]`.
fn forget_positions(table: &mut Table) {
    table.set_position(None);
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(t) => forget_positions(t),
            Item::ArrayOfTables(a) => a.iter_mut().for_each(forget_positions),
            _ => {}
        }
    }
}

fn value_text(value: &toml_edit::Value) -> String {
    let mut value = value.clone();
    *value.decor_mut() = toml_edit::Decor::default();
    value.to_string()
}

fn same_entries(inline: &toml_edit::InlineTable, table: &Table) -> bool {
    inline.len() == table.len()
        && inline.iter().all(|(key, value)| {
            table
                .get(key)
                .and_then(Item::as_value)
                .is_some_and(|other| value_text(value) == value_text(other))
        })
}

/// Makes `old` say what `new` says, touching only what differs: a value that did not change keeps its
/// comments, a changed one keeps the comment of its line, a key that is gone is removed.
fn merge_table(old: &mut Table, new: &Table) {
    let gone: Vec<String> = old
        .iter()
        .map(|(key, _)| key.to_owned())
        .filter(|key| !new.contains_key(key))
        .collect();
    for key in gone {
        old.remove(&key);
    }
    for (key, item) in new {
        match (old.get_mut(key), item) {
            (Some(Item::Table(o)), Item::Table(n)) => merge_table(o, n),
            // `token = {}` written by hand, against the `[destination.secrets.token]` that the
            // serializer writes: the same table, left as the user wrote it.
            (Some(Item::Value(toml_edit::Value::InlineTable(o))), Item::Table(n))
                if same_entries(o, n) => {}
            (Some(Item::Value(o)), Item::Value(n)) => {
                if value_text(o) != value_text(n) {
                    let mut replacement = n.clone();
                    *replacement.decor_mut() = o.decor().clone();
                    *o = replacement;
                }
            }
            (Some(slot), _) => {
                let mut replacement = item.clone();
                if let Item::Table(t) = &mut replacement {
                    forget_positions(t);
                }
                *slot = replacement;
            }
            (None, _) => {
                let mut added = item.clone();
                if let Item::Table(t) = &mut added {
                    forget_positions(t);
                }
                old.insert(key, added);
            }
        }
    }
}

/// The TOML text of the destination `id` as it is in the file, comments included, for an editor.
///
/// # Errors
/// [`UploadError::Config`] (`unknown_destination`) when there is none or the file is not valid,
/// [`UploadError::Io`] if the file cannot be read.
pub fn destination_toml(dir: &Path, id: &str) -> Result<String, UploadError> {
    let path = destinations_path(dir);
    let shown = path.display().to_string();
    let text = read(&path)?.unwrap_or_default();
    let doc = parse_document(&text, &shown)?;
    let index = index_of(&doc, id).ok_or_else(|| unknown(id))?;
    let mut table = doc
        .get("destination")
        .and_then(Item::as_array_of_tables)
        .and_then(|a| a.get(index))
        .cloned()
        .ok_or_else(|| unknown(id))?;
    forget_positions(&mut table);
    let mut out = DocumentMut::new();
    let mut array = ArrayOfTables::new();
    array.push(table);
    out.insert("destination", Item::ArrayOfTables(array));
    Ok(out.to_string())
}

/// Replaces the destination `destination.id` with `destination`, keeping the comments of the file
/// and of every line that did not change. The result is validated before it replaces the file.
///
/// # Errors
/// [`UploadError::Config`] (`unknown_destination`) if there is no such destination or the result is
/// not valid, [`UploadError::Io`] if the file cannot be read or written.
pub fn replace_destination(dir: &Path, destination: &Destination) -> Result<(), UploadError> {
    let path = destinations_path(dir);
    let shown = path.display().to_string();
    let text = read(&path)?.unwrap_or_default();
    let mut doc = parse_document(&text, &shown)?;
    let index = index_of(&doc, &destination.id).ok_or_else(|| unknown(&destination.id))?;
    let new_doc = parse_document(&destination.to_toml()?, &shown)?;
    let new = new_doc
        .get("destination")
        .and_then(Item::as_array_of_tables)
        .and_then(|a| a.get(0))
        .ok_or_else(|| UploadError::config("cannot write the destination"))?;
    let old = destinations_mut(&mut doc)
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| unknown(&destination.id))?;
    merge_table(old, new);
    let result = doc.to_string();
    DestinationFile::parse(&result, &shown)?;
    write(&path, &result)
}

/// Replaces the destination `id` with `edited`, the text of one destination with the same `id`
/// (what a user made of [`destination_toml`] in an editor): the comments of the edited text are kept,
/// those of the rest of the file too. The result is validated before it replaces the file.
///
/// # Errors
/// [`UploadError::Config`] if there is no such destination, `edited` is not one destination named
/// `id`, or the result is not valid; [`UploadError::Io`] if the file cannot be read or written.
pub fn replace_destination_text(dir: &Path, id: &str, edited: &str) -> Result<(), UploadError> {
    let path = destinations_path(dir);
    let shown = path.display().to_string();
    let text = read(&path)?.unwrap_or_default();
    let mut doc = parse_document(&text, &shown)?;
    let index = index_of(&doc, id).ok_or_else(|| unknown(id))?;
    let mut edited_doc = parse_document(edited, "the edited profile")?;
    let mut table = {
        let array = destinations_mut(&mut edited_doc)
            .filter(|a| a.len() == 1)
            .ok_or_else(|| UploadError::config("one destination was expected"))?;
        array
            .get(0)
            .cloned()
            .ok_or_else(|| UploadError::config("one destination was expected"))?
    };
    if table.get("id").and_then(Item::as_str) != Some(id) {
        return Err(UploadError::config(
            "the id of a destination cannot be changed here: its secrets are filed under it",
        ));
    }
    forget_positions(&mut table);
    *destinations_mut(&mut doc)
        .and_then(|a| a.get_mut(index))
        .ok_or_else(|| unknown(id))? = table;
    let result = doc.to_string();
    DestinationFile::parse(&result, &shown)?;
    write(&path, &result)
}

/// Removes the destination `id`, keeping the rest of the file as it is. Returns whether there was one.
///
/// # Errors
/// [`UploadError::Config`] if the file is not valid or the result is not, [`UploadError::Io`] if the
/// file cannot be read or written.
pub fn remove_destination(dir: &Path, id: &str) -> Result<bool, UploadError> {
    let path = destinations_path(dir);
    let shown = path.display().to_string();
    let Some(text) = read(&path)? else {
        return Ok(false);
    };
    let mut doc = parse_document(&text, &shown)?;
    let Some(index) = index_of(&doc, id) else {
        return Ok(false);
    };
    let now_empty = {
        let array = destinations_mut(&mut doc).ok_or_else(|| unknown(id))?;
        array.remove(index);
        array.is_empty()
    };
    if now_empty {
        doc.remove("destination");
    }
    let result = doc.to_string();
    DestinationFile::parse(&result, &shown)?;
    write(&path, &result)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        append_destination, destination_toml, destinations_path, find, load_destinations,
        remove_destination, replace_destination, replace_destination_text,
    };
    use crate::destination::{Destination, MediaKind};

    const COMMENTED: &str = "\
# my destinations
[[destination]]
# the main one
id = \"one\"
type = \"zipline\"
url = \"https://one.example.org\" # the server
kinds = [\"image\", \"file\"]

[destination.secrets]
token = {} # filled by 'secrets set'

# the second one
[[destination]]
id = \"two\"
type = \"zipline\"
url = \"https://two.example.org\"

[destination.secrets]
token = {}

[destination.options]
format = \"date\" # keep this
";

    fn commented() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(destinations_path(dir.path()), COMMENTED).unwrap();
        dir
    }

    fn text(dir: &tempfile::TempDir) -> String {
        std::fs::read_to_string(destinations_path(dir.path())).unwrap()
    }

    #[test]
    fn replace_changes_only_what_changed_and_keeps_every_comment() {
        let dir = commented();
        let mut one = find(&load_destinations(dir.path()).unwrap(), "one")
            .unwrap()
            .clone();
        one.url = Some("https://new.example.org".into());
        one.kinds = vec![MediaKind::Image];
        one.label = Some("Main".into());
        replace_destination(dir.path(), &one).unwrap();
        let after = text(&dir);
        for kept in [
            "# my destinations\n",
            "# the main one\n",
            "token = {} # filled by 'secrets set'",
            "# the second one\n",
            "format = \"date\" # keep this",
        ] {
            assert!(after.contains(kept), "{kept:?} lost:\n{after}");
        }
        assert!(
            after.contains("url = \"https://new.example.org\" # the server"),
            "{after}"
        );
        assert!(!after.contains("https://one.example.org"), "{after}");
        let all = load_destinations(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
        let one = find(&all, "one").unwrap();
        assert_eq!(one.url.as_deref(), Some("https://new.example.org"));
        assert_eq!(one.kinds, vec![MediaKind::Image]);
        assert_eq!(one.label.as_deref(), Some("Main"));
        // The other profile is the same, and its options are still its own.
        let two = find(&all, "two").unwrap();
        assert_eq!(two.url.as_deref(), Some("https://two.example.org"));
        assert_eq!(
            two.options.as_ref().unwrap().format.as_deref(),
            Some("date")
        );
        assert!(one.options.is_none());
    }

    #[test]
    fn replace_adds_a_table_under_the_right_profile() {
        let dir = commented();
        let mut one = find(&load_destinations(dir.path()).unwrap(), "one")
            .unwrap()
            .clone();
        one.options = Some(crate::destination::ZiplineOptions {
            format: Some("uuid".into()),
            original_name: false,
            folder: None,
        });
        replace_destination(dir.path(), &one).unwrap();
        let all = load_destinations(dir.path()).unwrap();
        assert_eq!(
            find(&all, "one")
                .unwrap()
                .options
                .as_ref()
                .unwrap()
                .format
                .as_deref(),
            Some("uuid")
        );
        assert_eq!(
            find(&all, "two")
                .unwrap()
                .options
                .as_ref()
                .unwrap()
                .format
                .as_deref(),
            Some("date")
        );
    }

    #[test]
    fn replace_of_an_unknown_or_invalid_destination_changes_nothing() {
        let dir = commented();
        let ghost = Destination::zipline("ghost", "https://g.example.org");
        let err = replace_destination(dir.path(), &ghost).unwrap_err();
        assert_eq!(err.exit_code().code(), 6);
        let mut bad = find(&load_destinations(dir.path()).unwrap(), "one")
            .unwrap()
            .clone();
        bad.url = Some("not a url".into());
        assert!(replace_destination(dir.path(), &bad).is_err());
        assert_eq!(text(&dir), COMMENTED);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn remove_takes_one_profile_and_keeps_the_rest_of_the_file() {
        let dir = commented();
        assert!(remove_destination(dir.path(), "one").unwrap());
        let after = text(&dir);
        assert!(after.contains("# the second one"), "{after}");
        assert!(after.contains("format = \"date\" # keep this"), "{after}");
        assert!(!after.contains("one.example.org"), "{after}");
        assert!(!remove_destination(dir.path(), "one").unwrap());
        assert!(remove_destination(dir.path(), "two").unwrap());
        assert!(load_destinations(dir.path()).unwrap().is_empty());
        // Nothing to remove in a missing file.
        let empty = tempfile::tempdir().unwrap();
        assert!(!remove_destination(empty.path(), "x").unwrap());
    }

    #[test]
    fn the_editor_text_is_the_block_of_the_profile_with_its_comments() {
        let dir = commented();
        let block = destination_toml(dir.path(), "one").unwrap();
        assert!(block.contains("# the main one"), "{block}");
        assert!(block.contains("# the server"), "{block}");
        assert!(!block.contains("two.example.org"), "{block}");
        assert!(destination_toml(dir.path(), "ghost").is_err());
        // The second profile's block reads back as a valid file of one destination.
        let block = destination_toml(dir.path(), "two").unwrap();
        let file = crate::destination::DestinationFile::parse(&block, "t").unwrap();
        assert_eq!(file.destination.len(), 1);
        assert_eq!(
            file.destination[0]
                .options
                .as_ref()
                .unwrap()
                .format
                .as_deref(),
            Some("date")
        );
    }

    #[test]
    fn replace_text_swaps_the_block_and_keeps_the_new_comments() {
        let dir = commented();
        let edited = destination_toml(dir.path(), "two")
            .unwrap()
            .replace("two.example.org", "other.example.org")
            + "";
        let edited = edited.replace("[destination.options]", "# my note\n[destination.options]");
        replace_destination_text(dir.path(), "two", &edited).unwrap();
        let after = text(&dir);
        assert!(after.contains("# my note"), "{after}");
        assert!(after.contains("# the main one"), "{after}");
        let all = load_destinations(dir.path()).unwrap();
        assert_eq!(
            find(&all, "two").unwrap().url.as_deref(),
            Some("https://other.example.org")
        );
        assert_eq!(
            find(&all, "one").unwrap().url.as_deref(),
            Some("https://one.example.org")
        );
    }

    #[test]
    fn replace_text_refuses_another_id_two_profiles_or_an_invalid_result() {
        let dir = commented();
        let block = destination_toml(dir.path(), "two").unwrap();
        for bad in [
            block.replace("id = \"two\"", "id = \"three\""),
            format!("{block}\n{}", destination_toml(dir.path(), "one").unwrap()),
            block.replace("https://two.example.org", "nope"),
            "not toml [".to_owned(),
            String::new(),
        ] {
            assert!(
                replace_destination_text(dir.path(), "two", &bad).is_err(),
                "{bad}"
            );
        }
        assert_eq!(text(&dir), COMMENTED);
    }

    #[test]
    fn missing_file_is_empty_and_append_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_destinations(dir.path()).unwrap().is_empty());
        append_destination(
            dir.path(),
            &Destination::zipline("one", "https://one.example.org"),
        )
        .unwrap();
        let path = destinations_path(dir.path());
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.insert_str(0, "# my destinations\n");
        std::fs::write(&path, text).unwrap();
        append_destination(
            dir.path(),
            &Destination::zipline("two", "https://two.example.org"),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# my destinations\n"));
        let all = load_destinations(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            find(&all, "two").unwrap().url.as_deref(),
            Some("https://two.example.org")
        );
        assert!(find(&all, "three").is_err());
        assert!(append_destination(dir.path(), &all[0]).is_err());
    }

    #[test]
    fn an_invalid_file_is_a_config_error_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(destinations_path(dir.path()), "[[destination]]\nid = 3\n").unwrap();
        let err = load_destinations(dir.path()).unwrap_err();
        assert_eq!(err.exit_code().code(), 6);
        assert!(err.to_string().contains("destinations.toml:"), "{err}");
    }
}
