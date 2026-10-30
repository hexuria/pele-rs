// Port of src/types.ts (the parts the port uses).

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LinkInfo {
    pub id: String,
    pub href: String,
    pub internal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub links: Vec<LinkInfo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderResult {
    pub type_: &'static str,
    pub mermaid_type: &'static str,
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub links: Vec<LinkInfo>,
}

/// The render options a diagram reads. The host passes none, so every other option of
/// upstream's `RenderOptions` is at its default (see PORTING.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    // False when the type's config section sets `useMaxWidth: false`.
    pub responsive: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { responsive: true }
    }
}
