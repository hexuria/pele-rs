// Port of src/diagrams/treeview/icons.ts.

use crate::preprocess::Config;
use crate::util::yaml::{Map, Value};

// Mermaid's name for its built-in pack. Only `file` and `folder` are built in.
pub const BUILTIN_PACK: &str = "mermaid-treeview";

// Outlines on a 24 by 24 grid.
pub fn glyph(name: &str) -> Option<&'static str> {
    match name {
        "folder" => Some("M3 6.5A1.5 1.5 0 0 1 4.5 5h4.6l2.4 2.5h8A1.5 1.5 0 0 1 21 9v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 18z"),
        "file" => Some("M6.5 3h7L19 8.5v11a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 5 19.5v-15A1.5 1.5 0 0 1 6.5 3zM13.5 3v5.5H19"),
        _ => None,
    }
}

pub struct IconConfig {
    pub show_icons: bool,
    pub default_icon_pack: String,
    pub filename_icons: Option<Map>,
    pub extension_icons: Option<Map>,
}

fn own(map: Option<&Map>, key: &str) -> Option<String> {
    match map?.get(key)? {
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// A file's icon from the configured maps. A file name wins over an extension, and extensions
/// match in lower case with or without the leading dot.
pub fn detect_icon(name: &str, config: &IconConfig) -> Option<String> {
    if let Some(by_name) = own(config.filename_icons.as_ref(), name).filter(|s| !s.is_empty()) {
        return Some(by_name);
    }
    let units: Vec<u16> = name.encode_utf16().collect();
    let dot = units.iter().rposition(|&c| c == 46)?;
    if dot == 0 {
        return None;
    }
    let ext = String::from_utf16_lossy(&units[dot..]).to_lowercase();
    own(config.extension_icons.as_ref(), &ext).or_else(|| own(config.extension_icons.as_ref(), &ext[1..]))
}

fn qualify_icon(icon: &str, default_icon_pack: &str) -> String {
    if icon.contains(':') {
        return icon.to_string();
    }
    if glyph(icon).is_some() || default_icon_pack.is_empty() {
        return format!("{}:{}", BUILTIN_PACK, icon);
    }
    format!("{}:{}", default_icon_pack, icon)
}

/// The qualified icon name for a node, or None when it shows none. An `icon()` annotation
/// always wins; otherwise icons appear only with `showIcons`.
pub fn get_node_icon(icon: Option<&str>, name: &str, directory: bool, config: &IconConfig) -> Option<String> {
    if icon == Some("none") {
        return None;
    }
    if let Some(i) = icon.filter(|i| !i.is_empty()) {
        return Some(qualify_icon(i, &config.default_icon_pack));
    }
    if !config.show_icons {
        return None;
    }
    if !directory {
        let detected = detect_icon(name, config);
        if detected.as_deref() == Some("none") {
            return None;
        }
        if let Some(d) = detected.filter(|d| !d.is_empty()) {
            return Some(qualify_icon(&d, &config.default_icon_pack));
        }
    }
    Some(format!("{}:{}", BUILTIN_PACK, if directory { "folder" } else { "file" }))
}

pub fn icon_config(config: &Config) -> IconConfig {
    let c = config.get("treeView").and_then(|v| v.as_map());
    let get = |k: &str| c.and_then(|c| c.get(k));
    IconConfig {
        show_icons: get("showIcons").is_some_and(|v| v.truthy()),
        default_icon_pack: match get("defaultIconPack") {
            Some(Value::Str(s)) => s.clone(),
            _ => String::new(),
        },
        filename_icons: get("filenameIcons").and_then(|v| v.as_map()).cloned(),
        extension_icons: get("extensionIcons").and_then(|v| v.as_map()).cloned(),
    }
}
