//! The projection of a status onto one field class, which the domain and control digests are
//! computed over.

use super::EncodeError;
use super::cbor::Value;
use crate::fields::{FieldClass, FieldClasses, FieldPath, Segment, partition};
use std::collections::BTreeMap;

/// The declared fields of a status type as a tree of paths.
#[derive(Default)]
struct Node {
    /// The class declared at this path: a leaf's class, or the shape class of an `Option`
    /// or of a container's members.
    class: Option<FieldClass>,
    /// The named fields under this path, by [`fold`]ed name, with their declared name.
    fields: BTreeMap<String, (&'static str, Node)>,
    /// The members of a list, ring or map at this path.
    each: Option<Box<Node>>,
}

impl Node {
    fn is_leaf(&self) -> bool {
        self.fields.is_empty() && self.each.is_none()
    }

    /// Whether a present but otherwise empty value at this path still counts in `class`.
    fn keeps_empty(&self, class: FieldClass) -> bool {
        self.class == Some(class)
    }
}

/// A declared or serialized field name with underscores removed and ASCII case folded, so that
/// serde's snake-case and camel-case renames of a field match its declared name.
fn fold(name: &str) -> String {
    name.chars()
        .filter(|&c| c != '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The tree of `S`'s declared fields.
fn tree<S: FieldClasses>() -> Result<Node, EncodeError> {
    let mut root = Node::default();
    for (path, class) in partition::<S>() {
        let mut node = &mut root;
        for segment in path.segments() {
            node = match *segment {
                Segment::Field(name) => {
                    let entry = node
                        .fields
                        .entry(fold(name))
                        .or_insert_with(|| (name, Node::default()));
                    if entry.0 != name {
                        return Err(EncodeError::AmbiguousField(path.to_string()));
                    }
                    &mut entry.1
                }
                Segment::Each => node.each.get_or_insert_with(Box::default),
            };
        }
        node.class = Some(class);
    }
    Ok(root)
}

/// The part of `status`, serialized as `value`, whose fields are of `class`, as a map.
///
/// # Errors
///
/// [`EncodeError::AmbiguousField`] if two declared fields fold to the same name,
/// [`EncodeError::UnknownField`] if the serialized status holds a field `S` does not declare,
/// and [`EncodeError::Shape`] if a serialized value does not have the shape its declaration
/// implies.
pub(super) fn project<S: FieldClasses>(
    value: &Value,
    class: FieldClass,
) -> Result<Value, EncodeError> {
    let root = tree::<S>()?;
    Ok(project_node(value, &root, class, &FieldPath::root())?
        .unwrap_or_else(|| Value::Map(Vec::new())))
}

fn project_node(
    value: &Value,
    node: &Node,
    class: FieldClass,
    path: &FieldPath,
) -> Result<Option<Value>, EncodeError> {
    if node.is_leaf() {
        return Ok((node.class == Some(class)).then(|| value.clone()));
    }
    if *value == Value::Null {
        return Ok(node.keeps_empty(class).then_some(Value::Null));
    }
    if let Some(each) = &node.each {
        return project_members(value, each, class, &path.each());
    }
    let Value::Map(entries) = value else {
        return Err(EncodeError::Shape(path.to_string()));
    };
    let mut out = Vec::new();
    for (key, field) in entries {
        let Value::Text(name) = key else {
            return Err(EncodeError::Shape(path.to_string()));
        };
        let Some((declared, child)) = node.fields.get(&fold(name)) else {
            return Err(EncodeError::UnknownField(if path.segments().is_empty() {
                name.clone()
            } else {
                format!("{path}.{name}")
            }));
        };
        if let Some(v) = project_node(field, child, class, &path.field(declared))? {
            out.push((key.clone(), v));
        }
    }
    Ok((!out.is_empty() || node.keeps_empty(class)).then_some(Value::Map(out)))
}

/// The projection of a list, ring or keyed map whose members are declared by `each`.
fn project_members(
    value: &Value,
    each: &Node,
    class: FieldClass,
    path: &FieldPath,
) -> Result<Option<Value>, EncodeError> {
    let keep = each.keeps_empty(class);
    match value {
        Value::Array(items) => {
            let projected = items
                .iter()
                .map(|item| project_node(item, each, class, path))
                .collect::<Result<Vec<_>, _>>()?;
            if !keep && projected.iter().all(Option::is_none) {
                return Ok(None);
            }
            Ok(Some(Value::Array(
                projected
                    .into_iter()
                    .map(|v| v.unwrap_or(Value::Null))
                    .collect(),
            )))
        }
        Value::Map(entries) => {
            let mut out = Vec::new();
            for (key, member) in entries {
                if let Some(v) = project_node(member, each, class, path)? {
                    out.push((key.clone(), v));
                }
            }
            Ok((!out.is_empty() || keep).then_some(Value::Map(out)))
        }
        _ => Err(EncodeError::Shape(path.to_string())),
    }
}
