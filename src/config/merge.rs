//! Saving the config without destroying what this build doesn't know about.
//!
//! `config.toml` is resolved from a shared location, so it can hold keys this
//! build has never declared — a newer or older Akagi's, or the user's own
//! notes. Loading drops them (`#[serde(default)]` ignores unknown keys), so
//! serialising the struct back over the file would delete them for good.
//!
//! Instead: parse the file as a `toml_edit` document, write only the keys this
//! build produces, and leave every other key, comment and blank line where it
//! is.
//!
//! Known limit: a section the user hand-wrote as an *inline* table
//! (`bot = { enabled = true }`) is replaced wholesale rather than merged, so
//! unknown keys inside it are still lost. Every file Akagi writes uses header
//! tables, so this only bites a hand-written file that also mixes in keys we
//! don't declare.

use toml_edit::{DocumentMut, Item, Table, Value};

/// Dotted paths whose tables are replaced wholesale rather than merged
/// key-by-key.
///
/// These hold user-supplied entries keyed by name rather than a fixed set of
/// fields: merging them would make a deleted entry immortal, because there is
/// no key left in the new document to overwrite the old one with.
///
/// Every map-like (`HashMap`/`BTreeMap`) field of `AppConfig` belongs here.
/// Currently that is the per-decision-kind log-normal table of the autoplay
/// delay model.
const OPAQUE_TABLES: [&[&str]; 1] = [&["autoplay", "delay", "lognormal"]];

/// Serialise `config` into `existing` (TOML text, possibly empty), returning
/// the merged document.
///
/// Fails only when `existing` is not valid TOML — there is nothing to merge
/// into then, and the caller decides whether to rewrite the file.
pub fn merge_into<T: serde::Serialize>(config: &T, existing: &str) -> Result<String, String> {
    let mut doc: DocumentMut = existing.parse().map_err(|e| format!("{e}"))?;
    let fresh: DocumentMut = toml::to_string_pretty(config)
        .map_err(|e| format!("{e}"))?
        .parse()
        .map_err(|e| format!("{e}"))?;
    merge_table(doc.as_table_mut(), fresh.as_table(), &mut Vec::new());
    Ok(doc.to_string())
}

/// Copy every key of `fresh` into `target`, recursing into sub-tables so
/// unknown keys inside a known table survive.
fn merge_table(target: &mut Table, fresh: &Table, path: &mut Vec<String>) {
    for (key, fresh_item) in fresh.iter() {
        path.push(key.to_string());

        // Recurse only where both sides are header tables and the table isn't
        // one we replace wholesale; anything else is a plain overwrite.
        let recurse = !is_opaque(path)
            && fresh_item.is_table()
            && target.get(key).is_some_and(Item::is_table);
        if recurse {
            if let (Item::Table(fresh_sub), Some(Item::Table(target_sub))) =
                (fresh_item, target.get_mut(key))
            {
                merge_table(target_sub, fresh_sub, path);
            }
        } else {
            replace_key(target, key, fresh_item);
        }

        path.pop();
    }
}

fn is_opaque(path: &[String]) -> bool {
    OPAQUE_TABLES
        .iter()
        .any(|p| p.len() == path.len() && p.iter().zip(path).all(|(a, b)| *a == b.as_str()))
}

/// Overwrite `key` in `target` with `fresh_item`, carrying over the
/// formatting the old entry had.
fn replace_key(target: &mut Table, key: &str, fresh_item: &Item) {
    let mut next = fresh_item.clone();
    keep_decor(target.get(key), &mut next);
    // The key's decor carries the comment *lines* above it (the value's
    // carries the trailing one) and `insert` rebuilds the key, so save and
    // restore it.
    let key_decor = target.key(key).map(|k| k.leaf_decor().clone());
    target.insert(key, next);
    if let (Some(decor), Some(mut k)) = (key_decor, target.key_mut(key)) {
        *k.leaf_decor_mut() = decor;
    }
}

/// Carry an entry's existing formatting onto its replacement: a scalar's
/// trailing comment, or a table's header comment and place in the file.
fn keep_decor(old: Option<&Item>, next: &mut Item) {
    match (old, &mut *next) {
        (Some(Item::Value(old_value)), Item::Value(new_value)) => {
            let decor = old_value.decor().clone();
            *new_value = std::mem::replace(new_value, Value::from(false)).decorated(
                decor.prefix().and_then(|p| p.as_str()).unwrap_or(" "),
                decor.suffix().and_then(|s| s.as_str()).unwrap_or(""),
            );
        }
        // A dotted table (`delay.mode = "lua"`) has no header of its own, so
        // its decor doesn't transfer to the header table replacing it.
        (Some(Item::Table(old_table)), Item::Table(new_table)) if !old_table.is_dotted() => {
            // Comments above `[section]` live on the table, and its document
            // position is what keeps the section where it already was instead
            // of teleporting it to the end of the file on every save.
            *new_table.decor_mut() = old_table.decor().clone();
            new_table.set_position(old_table.position());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::AppConfig;
    use super::*;
    use serde::Serialize;

    /// Scalars must be declared before the nested table — the TOML serialiser
    /// rejects a value that follows a table.
    #[derive(Serialize)]
    struct Cfg {
        top: u32,
        section: Section,
    }

    #[derive(Serialize)]
    struct Section {
        flag: bool,
        name: String,
    }

    fn cfg() -> Cfg {
        Cfg {
            top: 7,
            section: Section {
                flag: true,
                name: "new".to_string(),
            },
        }
    }

    /// A whole section this build never declared stays in the file, while our
    /// own keys are updated.
    #[test]
    fn unknown_section_survives() {
        let existing = "\
top = 1

[section]
flag = false
name = \"old\"

[their_section]
keep = \"me\"
count = 3
";
        let out = merge_into(&cfg(), existing).unwrap();

        assert!(
            out.contains("[their_section]\nkeep = \"me\"\ncount = 3\n"),
            "foreign section must survive byte-identical:\n{out}"
        );
        assert!(out.contains("top = 7"), "{out}");
        assert!(out.contains("flag = true"), "{out}");
        assert!(out.contains("name = \"new\""), "{out}");
    }

    /// The case a table-level overwrite loses: an unknown key *inside* a
    /// section we do declare.
    #[test]
    fn unknown_key_inside_known_section_survives() {
        let existing = "\
top = 1

[section]
flag = false
future_knob = 42
";
        let out = merge_into(&cfg(), existing).unwrap();

        assert!(out.contains("future_knob = 42"), "{out}");
        assert!(out.contains("flag = true"), "{out}");
    }

    /// Comments and blank lines the user wrote are formatting, not data we
    /// own — a save must not eat them.
    #[test]
    fn comments_and_formatting_survive() {
        let existing = "\
# Akagi config, hand-tuned

top = 1

[section]
# why this is off
flag = false # keep an eye on this
name = \"old\"
";
        let out = merge_into(&cfg(), existing).unwrap();

        assert!(out.contains("# Akagi config, hand-tuned"), "{out}");
        assert!(out.contains("# why this is off"), "{out}");
        assert!(out.contains("flag = true # keep an eye on this"), "{out}");
    }

    /// Nothing on disk yet (or an empty file): the merge is just the config.
    #[test]
    fn empty_document_gets_full_config() {
        let out = merge_into(&cfg(), "").unwrap();

        let back: toml::Value = toml::from_str(&out).unwrap();
        assert_eq!(back["top"].as_integer(), Some(7));
        assert_eq!(back["section"]["flag"].as_bool(), Some(true));
        assert_eq!(back["section"]["name"].as_str(), Some("new"));
    }

    /// Saving twice in a row must produce identical bytes, otherwise every
    /// save churns the file (and its git history, for anyone tracking it).
    #[test]
    fn merge_is_idempotent() {
        let existing = "\
# header

top = 1

[their_section]
keep = \"me\"

[section]
flag = false
future_knob = 42
";
        let once = merge_into(&cfg(), existing).unwrap();
        let twice = merge_into(&cfg(), &once).unwrap();

        assert_eq!(once, twice, "second save changed the file");
    }

    /// Map-like tables are replaced wholesale: an entry removed from the map
    /// must not come back, or a user could never delete one.
    #[test]
    fn opaque_table_drops_removed_entries() {
        let existing = toml::to_string_pretty(&AppConfig::default()).unwrap();
        assert!(
            existing.contains("[autoplay.delay.lognormal]"),
            "test assumes the map is a header table:\n{existing}"
        );

        let mut cfg = AppConfig::default();
        cfg.autoplay.delay.lognormal.remove("hora");
        cfg.autoplay
            .delay
            .lognormal
            .insert("reach".to_string(), [2.0, 0.5]);

        let out = merge_into(&cfg, &existing).unwrap();
        let back: AppConfig = toml::from_str(&out).unwrap();

        assert!(
            !back.autoplay.delay.lognormal.contains_key("hora"),
            "a removed map entry came back:\n{out}"
        );
        assert_eq!(back.autoplay.delay.lognormal["reach"], [2.0, 0.5]);
        assert!(back.autoplay.delay.lognormal.contains_key("dahai_tedashi"));
    }

    /// Replacing a map-like table keeps its header comment and its place in
    /// the file rather than moving it to the end.
    #[test]
    fn opaque_table_keeps_its_comment_and_place() {
        let existing = "\
[autoplay.delay]
# calibrated by hand, do not touch
[autoplay.delay.lognormal]
reach = [1.0, 0.5]

[overlay]
enabled = true
";
        let out = merge_into(&AppConfig::default(), existing).unwrap();

        assert!(
            out.contains("# calibrated by hand, do not touch\n[autoplay.delay.lognormal]"),
            "{out}"
        );
        assert!(
            out.find("[autoplay.delay.lognormal]") < out.find("[overlay]"),
            "the table moved:\n{out}"
        );
    }

    /// The real thing: a config file holding a section from some other build
    /// round-trips through a save with that section untouched.
    #[test]
    fn app_config_save_preserves_foreign_section() {
        let existing = "\
# my notes

[general]
first_run_completed = true

[experimental_v4]
feature = \"on\"
level = 3
";
        let cfg: AppConfig = toml::from_str(existing).unwrap();
        let out = merge_into(&cfg, existing).unwrap();

        assert!(
            out.contains("[experimental_v4]\nfeature = \"on\"\nlevel = 3\n"),
            "foreign section must survive byte-identical:\n{out}"
        );
        assert!(out.contains("# my notes"), "{out}");

        let back: AppConfig = toml::from_str(&out).unwrap();
        assert!(back.general.first_run_completed);
        // ...and the sections this build owns are all present now.
        assert!(out.contains("[bot]"), "{out}");
        assert!(out.contains("[overlay]"), "{out}");
    }

    /// A hand-written file may use dotted keys where we emit header tables
    /// (`delay.mode = "legacy"` inside `[autoplay]`). Merging into a dotted
    /// table must still produce a document that parses, with the user's value
    /// updated and everything else intact.
    #[test]
    fn dotted_keys_in_the_existing_file_still_merge() {
        let existing = "\
[autoplay]
enabled = true
delay.mode = \"legacy\"
delay.some_future_knob = 1
";
        let mut cfg = AppConfig::default();
        cfg.autoplay.enabled = true;
        cfg.autoplay.delay.min_delay_ms = super::super::MsRange::new(1234, 1500);

        let out = merge_into(&cfg, existing).unwrap();
        let back: AppConfig = toml::from_str(&out).unwrap();

        assert!(back.autoplay.enabled);
        assert_eq!(
            back.autoplay.delay.min_delay_ms,
            super::super::MsRange::new(1234, 1500)
        );
        assert_eq!(back.autoplay.delay.mode, super::super::DelayMode::Lua);
        assert!(
            out.contains("delay.some_future_knob = 1"),
            "unknown dotted key was dropped:\n{out}"
        );
        assert_eq!(
            merge_into(&cfg, &out).unwrap(),
            out,
            "second save changed the file"
        );
    }

    /// Invalid TOML has no document to merge into; the caller needs the error
    /// rather than a silently mangled file.
    #[test]
    fn invalid_existing_document_is_an_error() {
        assert!(merge_into(&cfg(), "this is not = = toml").is_err());
    }
}
