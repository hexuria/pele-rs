// Port of src/diagrams/usecase/db.ts: resolves what the parser read into a model, once the whole
// diagram is read: which ids are actors, use cases, boundaries, JSON nodes or edges, in what order
// they appear, and whether the statements about them agree. Ports Mermaid's UsecaseModelBuilder,
// with its messages.

use super::json::Json;
use super::lexer::{line_column, location_text};
use super::{Actor, Boundary, JsonNode, Note, OMap, Relationship, UseCase, UsecaseModel};
use crate::errors::{PeleError, PeleErrorCode};

pub type Span = (usize, usize);

#[derive(Clone)]
pub struct DraftLabel {
    pub text: String,
    pub markdown: bool,
    pub span: Span,
}

#[derive(Clone, PartialEq)]
pub enum PropertyValue {
    Str(String),
    Bool(bool),
}

// keySpan is also where an error about the property points.
#[derive(Clone)]
pub struct DraftProperty {
    pub key: String,
    pub value: PropertyValue,
    pub key_span: Span,
}

pub struct DraftElement {
    pub id: String,
    pub kind: &'static str,
    pub label: DraftLabel,
    pub span: Span,
    pub generated: bool,
    // Index into Drafts::boundaries.
    pub parent: Option<usize>,
    pub shape: Option<&'static str>,
    pub metadata: Option<Vec<DraftProperty>>,
    pub stereotype: Option<String>,
    pub classes: Vec<String>,
}

pub struct DraftBoundary {
    pub id: String,
    pub label: DraftLabel,
    pub span: Span,
    pub generated: bool,
    pub classes: Vec<String>,
}

pub struct DraftJson {
    pub id: String,
    pub value: Vec<(String, Json)>,
    pub span: Span,
    pub classes: Vec<String>,
}

pub struct DraftEndpoint {
    pub id: String,
    pub label: DraftLabel,
    pub span: Span,
    pub generated: bool,
    // Whether this mention declares the element, and whether it carries a `:::` suffix.
    pub declares: bool,
    pub classed: bool,
}

pub struct DraftRelationship {
    pub source: DraftEndpoint,
    pub target: DraftEndpoint,
    pub span: Span,
    pub id: Option<String>,
    pub id_span: Option<Span>,
    pub type_: &'static str,
    pub arrow_type: u8,
    pub label: Option<DraftLabel>,
    pub minlen: f64,
}

// Everything the parser read, in source order, before any of it is resolved.
#[derive(Default)]
pub struct Drafts {
    pub elements: Vec<DraftElement>,
    pub boundaries: Vec<DraftBoundary>,
    pub jsons: Vec<DraftJson>,
    pub relationships: Vec<DraftRelationship>,
    // (target, span, label)
    pub notes: Vec<(String, Span, DraftLabel)>,
    // (target, span, properties)
    pub metadata: Vec<(String, Span, Vec<DraftProperty>)>,
    // (targets, classes)
    pub classes: Vec<(Vec<(String, Span)>, Vec<String>)>,
    // (target, span, styles)
    pub styles: Vec<(String, Span, Vec<String>)>,
    // (ids, styles)
    pub class_defs: Vec<(Vec<String>, Vec<String>)>,
    pub direction: Option<String>,
    pub acc_title: String,
    pub acc_description: String,
}

#[derive(Clone)]
struct Origin {
    kind: &'static str,
    span: Span,
    generated: bool,
    // The label a generated id was derived from.
    label: Option<String>,
}

#[derive(Clone)]
struct ElementState {
    kind: &'static str,
    id: String,
    label: DraftLabel,
    span: Span,
    parent: Option<usize>,
    shape: Option<&'static str>,
    stereotype: Option<String>,
    stereotype_span: Option<Span>,
    classes: Vec<String>,
    actor_type: Option<String>,
    icon: Option<String>,
    business: Option<bool>,
}

struct BoundaryState {
    id: String,
    label: DraftLabel,
    span: Span,
    classes: Vec<String>,
    type_: Option<&'static str>,
    members: Vec<String>,
}

fn push_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

// Which kind of element the keys of a metadata block could only belong to, for the error message.
fn infer_kind(properties: &[DraftProperty]) -> Option<&'static str> {
    let mut possible: Vec<&'static str> = vec!["actor", "usecase", "boundary", "edge"];
    for p in properties {
        match p.key.as_str() {
            "icon" => possible = vec!["actor"],
            "animate" | "animation" => possible = vec!["edge"],
            "type" => {
                possible = match &p.value {
                    PropertyValue::Str(v) if v == "rect" || v == "package" => vec!["boundary"],
                    PropertyValue::Str(v) if v == "normal" || v == "hollow" || v == "awesome" => vec!["actor"],
                    _ => Vec::new(),
                }
            }
            "business" => possible.retain(|k| *k != "boundary" && *k != "edge"),
            _ => return None,
        }
    }
    (possible.len() == 1).then(|| possible[0])
}

struct Resolver<'a> {
    src: &'a [u16],
    symbols: OMap<Origin>,
}

impl<'a> Resolver<'a> {
    fn at(&self, span: Span) -> String {
        location_text(self.src, span.0, span.1)
    }

    fn error(&self, message: String, span: Span) -> PeleError {
        let (line, column) = line_column(self.src, span.0);
        let mut e = PeleError::typed(message, PeleErrorCode::Semantic, "usecase");
        e.line = line;
        e.column = column;
        e
    }

    fn fail(&self, message: &str, span: Span) -> PeleError {
        self.error(format!("{} at {}", message, self.at(span)), span)
    }

    fn conflict(&self, message: &str, current: Span, previous: Span, current_label: Option<&str>, previous_label: Option<&str>) -> PeleError {
        let suffix = |label: Option<&str>| label.map_or(String::new(), |l| format!(" (label \"{}\")", l));
        self.error(format!("{} at {}{}; previous declaration at {}{}", message, self.at(current), suffix(current_label), self.at(previous), suffix(previous_label)), current)
    }

    fn invalid(&self, id: &str, kind: &str, property: &DraftProperty) -> PeleError {
        self.fail(&format!("Metadata property '{}' is invalid for {} '{}'", property.key, kind, id), property.key_span)
    }

    fn register(&mut self, id: &str, label: &DraftLabel, span: Span, generated: bool, kind: &'static str) {
        self.symbols.set(id, Origin { kind, span, generated, label: Some(label.text.clone()) });
    }

    // An id means one thing in the whole diagram, and an id made from a label may be declared once.
    fn claim(&self, id: &str, label: &DraftLabel, span: Span, generated: bool, kind: &str) -> Result<(), PeleError> {
        let Some(origin) = self.symbols.get(id) else { return Ok(()) };
        let label = if generated { Some(label.text.as_str()) } else { None };
        let origin_label = if origin.generated { origin.label.as_deref() } else { None };
        if origin.kind != kind {
            return Err(self.conflict(&format!("ID '{}' is declared as both {} and {}", id, origin.kind, kind), span, origin.span, label, origin_label));
        }
        if origin.generated || generated {
            return Err(self.conflict(&format!("Generated ID '{}' collides with another declaration", id), span, origin.span, label, origin_label));
        }
        Ok(())
    }

    fn unique(&mut self, id: &str, kind: &'static str, span: Span) -> Result<(), PeleError> {
        if let Some(previous) = self.symbols.get(id) {
            return Err(self.conflict(
                &format!("ID '{}' is declared more than once ({} and {})", id, previous.kind, kind),
                span,
                previous.span,
                None,
                if previous.generated { previous.label.as_deref() } else { None },
            ));
        }
        self.symbols.set(id, Origin { kind, span, generated: false, label: None });
        Ok(())
    }

    // A metadata statement replaces what a declaration said; two declarations have to agree.
    fn actor_property(&self, state: &mut ElementState, property: &DraftProperty, replace: bool) -> Result<(), PeleError> {
        let key = property.key.as_str();
        let clash = |previous: Option<PropertyValue>| -> Result<(), PeleError> {
            if !replace && previous.is_some_and(|p| p != property.value) {
                return Err(self.conflict(&format!("Actor '{}' has conflicting {} metadata", state.id, key), property.key_span, state.span, None, None));
            }
            Ok(())
        };
        match (&property.value, key) {
            (PropertyValue::Str(v), "type") if v == "normal" || v == "hollow" || v == "awesome" => {
                clash(state.actor_type.clone().map(PropertyValue::Str))?;
                state.actor_type = Some(v.clone());
            }
            (PropertyValue::Str(v), "icon") => {
                clash(state.icon.clone().map(PropertyValue::Str))?;
                state.icon = Some(v.clone());
            }
            (PropertyValue::Bool(b), "business") => {
                clash(state.business.map(PropertyValue::Bool))?;
                state.business = Some(*b);
            }
            _ => return Err(self.invalid(&state.id, "actor", property)),
        }
        Ok(())
    }

    fn element_metadata(&self, state: &mut ElementState, properties: Option<&[DraftProperty]>, replace: bool) -> Result<(), PeleError> {
        for property in properties.unwrap_or(&[]) {
            if state.kind == "actor" {
                self.actor_property(state, property, replace)?;
            } else if let (PropertyValue::Bool(b), "business") = (&property.value, property.key.as_str()) {
                if !replace && state.business.is_some_and(|x| x != *b) {
                    return Err(self.conflict(&format!("Use case '{}' has conflicting business metadata", state.id), property.key_span, state.span, None, None));
                }
                state.business = Some(*b);
            } else {
                return Err(self.invalid(&state.id, "usecase", property));
            }
        }
        Ok(())
    }
}

fn publish(model: &mut UsecaseModel, state: &ElementState, boundaries: &[DraftBoundary]) {
    let parent_id = state.parent.map(|p| boundaries[p].id.clone()).filter(|id| !id.is_empty());
    let stereotype = state.stereotype.clone().filter(|s| !s.is_empty());
    if state.kind == "actor" {
        model.use_cases.delete(&state.id);
        let icon = state.icon.clone().filter(|i| !i.is_empty());
        let actor = Actor {
            id: state.id.clone(),
            label: state.label.text.clone(),
            markdown: state.label.markdown,
            business: state.business.unwrap_or(false),
            classes: state.classes.clone(),
            type_: if icon.is_some() { "icon".to_string() } else { state.actor_type.clone().unwrap_or_else(|| "normal".into()) },
            styles: model.actors.get(&state.id).map_or(Vec::new(), |a| a.styles.clone()),
            icon,
            stereotype,
            parent_id,
        };
        model.actors.set(&state.id, actor);
    } else {
        model.actors.delete(&state.id);
        let use_case = UseCase {
            id: state.id.clone(),
            label: state.label.text.clone(),
            markdown: state.label.markdown,
            business: state.business.unwrap_or(false),
            classes: state.classes.clone(),
            shape: state.shape.unwrap_or("ellipse"),
            styles: model.use_cases.get(&state.id).map_or(Vec::new(), |u| u.styles.clone()),
            stereotype,
            parent_id,
        };
        model.use_cases.set(&state.id, use_case);
    }
}

fn in_order<'b, V>(values: impl Iterator<Item = &'b (String, V)>, first: &OMap<usize>) -> Vec<&'b V>
where
    V: 'b,
{
    let mut list: Vec<(&String, &V)> = values.map(|(k, v)| (k, v)).collect();
    list.sort_by_key(|(k, _)| first.get(k).copied().unwrap_or(0));
    list.into_iter().map(|(_, v)| v).collect()
}

pub fn resolve_usecase(src: &[u16], drafts: Drafts, title: Option<&str>) -> Result<UsecaseModel, PeleError> {
    let mut model = UsecaseModel::default();
    let mut r = Resolver { src, symbols: OMap::default() };
    let mut elements: OMap<ElementState> = OMap::default();
    let mut boundaries: OMap<BoundaryState> = OMap::default();
    // The offset at which each id is first mentioned, which decides the order of the model.
    let mut first: OMap<usize> = OMap::default();
    let mention = |first: &mut OMap<usize>, id: &str, offset: usize| {
        if first.get(id).is_none_or(|&p| offset < p) {
            first.set(id, offset);
        }
    };

    // Declarations, sorted by offset: (offset, kind, index).
    let mut declarations: Vec<(usize, u8, usize)> = Vec::new();
    for rel in &drafts.relationships {
        mention(&mut first, &rel.source.id, rel.source.span.0);
        mention(&mut first, &rel.target.id, rel.target.span.0);
    }
    for (i, d) in drafts.elements.iter().enumerate() {
        mention(&mut first, &d.id, d.span.0);
        declarations.push((d.span.0, 0, i));
    }
    for (i, d) in drafts.boundaries.iter().enumerate() {
        mention(&mut first, &d.id, d.span.0);
        declarations.push((d.span.0, 1, i));
    }
    for (i, d) in drafts.jsons.iter().enumerate() {
        mention(&mut first, &d.id, d.span.0);
        declarations.push((d.span.0, 2, i));
    }
    for (i, rel) in drafts.relationships.iter().enumerate() {
        if let (Some(id), Some(span)) = (&rel.id, rel.id_span) {
            if !id.is_empty() {
                declarations.push((span.0, 3, i));
            }
        }
    }
    declarations.sort_by_key(|d| d.0);
    for (_, kind, i) in declarations {
        match kind {
            0 => {
                let draft = &drafts.elements[i];
                r.claim(&draft.id, &draft.label, draft.span, draft.generated, draft.kind)?;
                let parent = draft.parent.filter(|&p| !drafts.boundaries[p].id.is_empty());
                match elements.get_mut(&draft.id) {
                    None => {
                        let mut state = ElementState {
                            kind: draft.kind,
                            id: draft.id.clone(),
                            label: draft.label.clone(),
                            span: draft.span,
                            parent,
                            shape: draft.shape,
                            stereotype: draft.stereotype.clone(),
                            stereotype_span: draft.stereotype.as_deref().filter(|s| !s.is_empty()).map(|_| draft.span),
                            classes: draft.classes.clone(),
                            actor_type: None,
                            icon: None,
                            business: None,
                        };
                        r.element_metadata(&mut state, draft.metadata.as_deref(), false)?;
                        elements.set(&draft.id, state);
                        r.register(&draft.id, &draft.label, draft.span, draft.generated, draft.kind);
                    }
                    Some(existing) => {
                        if existing.label.text != draft.label.text || existing.label.markdown != draft.label.markdown {
                            return Err(r.conflict(&format!("ID '{}' has conflicting labels", draft.id), draft.span, existing.span, None, None));
                        }
                        if let (Some(a), Some(b)) = (draft.shape, existing.shape) {
                            if a != b {
                                return Err(r.conflict(&format!("Use case '{}' has conflicting shapes", draft.id), draft.span, existing.span, None, None));
                            }
                        }
                        // A boundary whose title makes an empty id is no parent, but it still takes the place of one.
                        if let (Some(p), Some(e)) = (parent, existing.parent) {
                            let (pb, eb) = (&drafts.boundaries[p], &drafts.boundaries[e]);
                            if !eb.id.is_empty() && pb.id != eb.id {
                                return Err(r.conflict(&format!("Element '{}' belongs to more than one system boundary", draft.id), pb.span, eb.span, None, None));
                            }
                        }
                        if let (Some(a), Some(b)) = (draft.stereotype.as_deref().filter(|s| !s.is_empty()), existing.stereotype.as_deref().filter(|s| !s.is_empty())) {
                            if a != b {
                                return Err(r.conflict(&format!("Element '{}' has conflicting stereotypes", draft.id), draft.span, existing.stereotype_span.unwrap_or(existing.span), None, None));
                            }
                        }
                        if existing.shape.is_none() {
                            existing.shape = draft.shape;
                        }
                        if existing.parent.is_none() {
                            existing.parent = draft.parent;
                        }
                        if existing.stereotype.is_none() {
                            existing.stereotype = draft.stereotype.clone();
                        }
                        if existing.stereotype_span.is_none() {
                            existing.stereotype_span = draft.stereotype.as_deref().filter(|s| !s.is_empty()).map(|_| draft.span);
                        }
                        push_unique(&mut existing.classes, &draft.classes);
                        let mut state = existing.clone();
                        r.element_metadata(&mut state, draft.metadata.as_deref(), false)?;
                        elements.set(&draft.id, state);
                    }
                }
            }
            1 => {
                let draft = &drafts.boundaries[i];
                r.claim(&draft.id, &draft.label, draft.span, draft.generated, "boundary")?;
                match boundaries.get_mut(&draft.id) {
                    Some(existing) => {
                        if existing.label.text != draft.label.text || existing.label.markdown != draft.label.markdown {
                            return Err(r.conflict(&format!("Boundary '{}' has conflicting titles", draft.id), draft.span, existing.span, None, None));
                        }
                        push_unique(&mut existing.classes, &draft.classes);
                    }
                    None => {
                        boundaries
                            .set(&draft.id, BoundaryState { id: draft.id.clone(), label: draft.label.clone(), span: draft.span, classes: draft.classes.clone(), type_: None, members: Vec::new() });
                        r.register(&draft.id, &draft.label, draft.span, draft.generated, "boundary");
                    }
                }
            }
            2 => {
                let draft = &drafts.jsons[i];
                r.unique(&draft.id, "json", draft.span)?;
            }
            _ => {
                let rel = &drafts.relationships[i];
                r.unique(rel.id.as_deref().unwrap(), "edge", rel.id_span.unwrap())?;
            }
        }
    }

    for state in in_order(elements.0.iter(), &first) {
        publish(&mut model, state, &drafts.boundaries);
    }
    for state in in_order(boundaries.0.iter(), &first) {
        model.boundaries.set(
            &state.id,
            Boundary { id: state.id.clone(), label: state.label.text.clone(), markdown: state.label.markdown, type_: "rect", members: Vec::new(), classes: Vec::new(), styles: Vec::new() },
        );
    }
    let mut jsons: Vec<&DraftJson> = drafts.jsons.iter().collect();
    jsons.sort_by_key(|d| first.get(&d.id).copied().unwrap_or(0));
    for draft in jsons {
        model.json_nodes.set(&draft.id, JsonNode { id: draft.id.clone(), value: draft.value.clone(), classes: draft.classes.clone(), styles: Vec::new() });
    }

    let mut anonymous = 0;
    for draft in &drafts.relationships {
        let mut kinds = Vec::new();
        for end in [&draft.source, &draft.target] {
            // An end of a relationship that nothing declares becomes a use case.
            if end.classed && !end.declares {
                return Err(r.fail(&format!("Relationship endpoint '{}' uses ::: without declaring the node", end.id), end.span));
            }
            let kind = match r.symbols.get(&end.id) {
                Some(origin) => origin.kind,
                None => {
                    elements.set(
                        &end.id,
                        ElementState {
                            kind: "usecase",
                            id: end.id.clone(),
                            label: end.label.clone(),
                            span: end.span,
                            parent: None,
                            shape: Some("ellipse"),
                            stereotype: None,
                            stereotype_span: None,
                            classes: Vec::new(),
                            actor_type: None,
                            icon: None,
                            business: None,
                        },
                    );
                    r.register(&end.id, &end.label, end.span, end.generated, "usecase");
                    mention(&mut first, &end.id, end.span.0);
                    "usecase"
                }
            };
            kinds.push(kind);
        }
        let (source, target, type_) = (&draft.source, &draft.target, draft.type_);
        let (source_kind, target_kind) = (kinds[0], kinds[1]);
        let related = |k: &str| k == "actor" || k == "usecase" || k == "json";
        if !related(source_kind) {
            return Err(r.conflict(&format!("Relationship source '{}' cannot be {}", source.id, source_kind), source.span, r.symbols.get(&source.id).unwrap().span, None, None));
        }
        if !related(target_kind) {
            return Err(r.conflict(&format!("Relationship target '{}' cannot be {}", target.id, target_kind), target.span, r.symbols.get(&target.id).unwrap().span, None, None));
        }
        if (type_ == "include" || type_ == "extend") && (source_kind != "usecase" || target_kind != "usecase") {
            return Err(r.fail(&format!("{} relationship requires use-case endpoints", type_), draft.span));
        }
        if type_ == "generalization" && (source_kind == "json" || source_kind != target_kind) {
            return Err(r.fail("Generalization requires actor-to-actor or use-case-to-use-case endpoints", draft.span));
        }
        if type_ == "association" && (source_kind == "json" || target_kind == "json") && draft.arrow_type > 2 {
            return Err(r.fail(&format!("JSON relationship '{}' to '{}' permits only point, reversed-point, or markerless solid association", source.id, target.id), draft.span));
        }
        let explicit = draft.id.as_deref().is_some_and(|i| !i.is_empty());
        let id = if explicit {
            draft.id.clone().unwrap()
        } else {
            anonymous += 1;
            format!("edge-{}", anonymous - 1)
        };
        model.relationships.push(Relationship {
            id,
            explicit_id: explicit,
            source: source.id.clone(),
            target: target.id.clone(),
            type_,
            arrow_type: draft.arrow_type,
            label: draft.label.as_ref().map(|l| l.text.clone()),
            markdown: draft.label.as_ref().map(|l| l.markdown),
            minlen: draft.minlen,
            classes: Vec::new(),
            styles: Vec::new(),
            animate: false,
            animation: None,
        });
    }
    let edge_index = |model: &UsecaseModel, id: &str| model.relationships.iter().rposition(|r| r.id == id);

    // The relationships may have added use cases; put all elements back in the order of first mention.
    let declared_actors = std::mem::take(&mut model.actors);
    let declared_use_cases = std::mem::take(&mut model.use_cases);
    for state in in_order(elements.0.iter(), &first) {
        if let Some(actor) = declared_actors.get(&state.id) {
            model.actors.set(&state.id, actor.clone());
        } else if let Some(use_case) = declared_use_cases.get(&state.id) {
            model.use_cases.set(&state.id, use_case.clone());
        } else {
            publish(&mut model, state, &drafts.boundaries);
        }
    }

    for (target, span, properties) in &drafts.metadata {
        let Some(origin) = r.symbols.get(target) else {
            let inferred = infer_kind(properties);
            return Err(r.fail(&format!("Metadata target '{}' is unresolved{}", target, inferred.map_or(String::new(), |k| format!(" (metadata implies {})", k))), *span));
        };
        let kind = origin.kind;
        if kind == "actor" || kind == "usecase" {
            let mut state = elements.get(target).unwrap().clone();
            r.element_metadata(&mut state, Some(properties), true)?;
            elements.set(target, state);
            continue;
        }
        let edge = edge_index(&model, target);
        for property in properties {
            let (key, value) = (property.key.as_str(), &property.value);
            match (kind, key, value) {
                ("boundary", "type", PropertyValue::Str(v)) if (v == "rect" || v == "package") && boundaries.get(target).is_some() => {
                    boundaries.get_mut(target).unwrap().type_ = Some(if v == "rect" { "rect" } else { "package" });
                }
                ("edge", "animate", PropertyValue::Bool(b)) if edge.is_some() => model.relationships[edge.unwrap()].animate = *b,
                ("edge", "animation", PropertyValue::Str(v)) if (v == "fast" || v == "slow") && edge.is_some() => {
                    let rel = &mut model.relationships[edge.unwrap()];
                    rel.animation = Some(v.clone());
                    rel.animate = true;
                }
                _ => return Err(r.invalid(target, kind, property)),
            }
        }
    }
    for rel in model.relationships.iter_mut() {
        if rel.animation.is_some() {
            rel.animate = true;
        }
    }

    for (_, state) in &elements.0 {
        if state.kind == "actor" {
            let icon = state.icon.as_deref().is_some_and(|i| !i.is_empty());
            if let Some(t) = state.actor_type.as_deref().filter(|t| !t.is_empty()) {
                if icon && t != "normal" {
                    return Err(r.fail(&format!("Actor '{}' cannot combine icon with type '{}'", state.id, t), state.span));
                }
            }
            if state.business == Some(true) && (icon || state.actor_type.as_deref() == Some("awesome")) {
                return Err(r.fail(&format!("Business actor '{}' must use normal or hollow geometry", state.id), state.span));
            }
        } else if state.shape == Some("rect") && state.business == Some(true) {
            return Err(r.fail(&format!("Rectangular use case '{}' cannot be a business use case", state.id), state.span));
        }
        publish(&mut model, state, &drafts.boundaries);
    }

    let mut sorted: Vec<&DraftElement> = drafts.elements.iter().collect();
    sorted.sort_by_key(|d| d.span.0);
    for draft in sorted {
        let Some(p) = draft.parent else { continue };
        let pid = &drafts.boundaries[p].id;
        if pid.is_empty() {
            continue;
        }
        let members = &mut boundaries.get_mut(pid).unwrap().members;
        if !members.contains(&draft.id) {
            members.push(draft.id.clone());
        }
    }
    for (_, state) in &boundaries.0 {
        let b = model.boundaries.get_mut(&state.id).unwrap();
        b.type_ = state.type_.unwrap_or("rect");
        b.members = state.members.clone();
        b.classes = state.classes.clone();
    }

    let mut notes = 0;
    for (target, span, label) in &drafts.notes {
        let Some(origin) = r.symbols.get(target) else { return Err(r.fail(&format!("Note target '{}' is unresolved", target), *span)) };
        if origin.kind != "actor" && origin.kind != "usecase" {
            return Err(r.conflict(&format!("Note target '{}' must be an actor or use case, not {}", target, origin.kind), *span, origin.span, None, None));
        }
        model.notes.push(Note { id: format!("note-{}", notes), target: target.clone(), label: label.text.clone(), markdown: label.markdown });
        notes += 1;
    }

    for (ids, styles) in &drafts.class_defs {
        for id in ids {
            model.class_defs.set(id, styles.clone());
        }
    }
    // A class or style target: its classes and styles.
    fn stylable<'m>(model: &'m mut UsecaseModel, kind: Option<&str>, id: &str) -> Option<(&'m mut Vec<String>, &'m mut Vec<String>)> {
        match kind {
            Some("actor") => model.actors.get_mut(id).map(|a| (&mut a.classes, &mut a.styles)),
            Some("usecase") => model.use_cases.get_mut(id).map(|a| (&mut a.classes, &mut a.styles)),
            Some("boundary") => model.boundaries.get_mut(id).map(|a| (&mut a.classes, &mut a.styles)),
            Some("json") => model.json_nodes.get_mut(id).map(|a| (&mut a.classes, &mut a.styles)),
            _ => {
                let k = model.relationships.iter().rposition(|r| r.id == id)?;
                let r = &mut model.relationships[k];
                Some((&mut r.classes, &mut r.styles))
            }
        }
    }
    for (targets, classes) in &drafts.classes {
        for (id, span) in targets {
            let kind = r.symbols.get(id).map(|o| o.kind);
            match stylable(&mut model, kind, id) {
                Some((c, _)) => push_unique(c, classes),
                None => return Err(r.fail(&format!("Class/style target '{}' is unresolved or anonymous", id), *span)),
            }
        }
    }
    for (id, span, styles) in &drafts.styles {
        let kind = r.symbols.get(id).map(|o| o.kind);
        match stylable(&mut model, kind, id) {
            Some((_, s)) => s.extend(styles.iter().cloned()),
            None => return Err(r.fail(&format!("Class/style target '{}' is unresolved or anonymous", id), *span)),
        }
    }

    if let Some(d) = drafts.direction {
        model.direction = d;
    }
    model.title = title.map(str::to_string);
    model.acc_title = drafts.acc_title;
    model.acc_descr = crate::diagrams::cynefin::collapse_indents(&drafts.acc_description);
    Ok(model)
}
