//! Idempotent insert/remove of consumer-tagged objects inside a settings JSON
//! file.
//!
//! Every entry this module writes carries an `_ai_hooker_tag` marker so we can
//! find and remove our own work without disturbing entries the user added by
//! hand.
//!
//! The on-disk JSON's key order is preserved by enabling
//! `serde_json/preserve_order` in `Cargo.toml`.

mod common;
mod named_array;
mod named_object;
mod status_probe;
mod tagged_array;

pub(crate) use common::{read_or_empty, to_pretty};
pub(crate) use named_object::{
    contains_named, remove_named_object_entry, upsert_named_object_entry,
};
pub(crate) use status_probe::{tagged_hook_presence, tagged_hook_presence_for_event};
pub(crate) use tagged_array::{
    contains_tagged_array_entry_under, remove_tagged_array_entries_under, upsert_tagged_array_entry,
};

#[allow(unused_imports)]
pub(crate) use named_array::{
    contains_in_named_array, remove_named_array_entry, upsert_named_array_entry,
};
#[allow(unused_imports)]
pub(crate) use tagged_array::contains_tagged;
