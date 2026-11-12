//! Nothing in a package is read before the signature checks out, nothing written outside its
//! folder.

mod catalogue;
pub mod icon;
mod install;
mod manifest;
mod package;
pub mod packing;
mod permissions;

pub use catalogue::{
    catalogue, catalogue_entries, download, version_at_least, CatalogueEntry, CATALOGUE_KEY, INDEX, LEGACY_CORE, LEGACY_INDEX,
};
pub use install::{image_of, install, installed, recover, remove};
pub use manifest::{FieldKind, HookDeclaration, HookField, HookName, Kind, Localized, Manifest, GAMES_SINCE};
pub use package::{digest, open, replace_file, sign_package, Plugin, ENTRY_LIMIT, UNPACKED_LIMIT};
pub use permissions::{Permissions, Sending, Storage};
