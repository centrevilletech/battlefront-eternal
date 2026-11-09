//! createNativeAI's content snapshots. A side-table name, a definition id
//! and an owner reference are separate identities and stay separate here.
use super::AiInterner;
use super::*;
use crate::gen::*;
use crate::world::admit::AdmitError;
use bfe_content::value::Value;

pub struct ItemFacts<'a> {
    pub definition: &'a Value,
    pub id: DefId,
}

pub struct PostFacts<'a> {
    pub reference: SpawnPost,
    pub name: Option<&'a Value>,
    pub desc: &'a Value,
    pub ai: &'a Value,
}

/// The canonical identity lists from F3: items include embedded drops and
/// weapon items as well as named side items, even for repeated class names.
pub struct ContentFacts<'a> {
    pub classes: Vec<PreparedClass<'a>>,
    pub weapons: Vec<DefId>,
    pub weapon_data: Vec<AiWeaponData>,
    pub items: Vec<ItemFacts<'a>>,
    pub posts: Vec<PostFacts<'a>>,
    pub hulls: Vec<HullFacts<'a>>,
    pub props: Vec<PropFacts<'a>>,
}

impl Default for ContentFacts<'_> {
    fn default() -> Self {
        Self {
            classes: Vec::new(),
            weapons: Vec::new(),
            weapon_data: Vec::new(),
            items: Vec::new(),
            posts: Vec::new(),
            hulls: Vec::new(),
            props: Vec::new(),
        }
    }
}

/// Native typed facts are also useful to admission bodies that do not use
/// the boundary writer. The order here matches putAiWorldContent, so a
/// caller-supplied interner gets the same first-use order as the host.
pub fn content_rows(
    strings: &mut dyn AiInterner,
    facts: &ContentFacts<'_>,
) -> Result<AiWorldContent, AdmitError> {
    let classes = facts
        .classes
        .iter()
        .map(|v| class_facts(strings, v.id, v.definition, None, v.definition))
        .collect::<Result<_, _>>()?;
    let items = facts
        .items
        .iter()
        .map(|v| item_facts(strings, v.id, v.definition))
        .collect::<Result<_, _>>()?;
    let posts = facts
        .posts
        .iter()
        .map(|v| post_facts(strings, v.reference, v.name, v.desc, v.ai))
        .collect::<Result<_, _>>()?;
    let hulls = facts
        .hulls
        .iter()
        .map(|v| hull_facts(strings, v))
        .collect::<Result<_, _>>()?;
    let props = facts
        .props
        .iter()
        .map(|v| prop_facts(strings, v))
        .collect::<Result<_, _>>()?;
    Ok(AiWorldContent {
        classes,
        weapons: facts.weapons.clone(),
        weapon_data: facts.weapon_data.clone(),
        items,
        posts,
        hulls,
        props,
    })
}

fn content_call(content: &AiWorldContent) -> super::Call {
    encode(AiWorldContent::WORDS, |row, heap| content.put(row, heap))
}

/// Rust's Option intentionally has one absent value; the JS boundary has
/// separate null and undefined tags. Probe the generated writer to locate
/// an exceptional tag, rather than copying boundary field offsets here.
fn tag(
    call: &mut super::Call,
    content: &AiWorldContent,
    change: impl FnOnce(&mut AiWorldContent),
    tag: f64,
) -> Result<(), AdmitError> {
    let mut probe = content.clone();
    change(&mut probe);
    let probe = content_call(&probe);
    let base = content_call(content);
    let index = base
        .row
        .iter()
        .chain(&base.heap)
        .zip(probe.row.iter().chain(&probe.heap))
        .position(|(a, b)| a.to_bits() != b.to_bits())
        .ok_or_else(|| error("AI optional field has no generated tag"))?;
    if index < call.row.len() {
        call.row[index] = tag;
    } else {
        call.heap[index - call.row.len()] = tag;
    }
    Ok(())
}

fn label_is_null(identity: &Value) -> bool {
    matches!(
        identity
            .get("label")
            .filter(|v| !v.is_null())
            .or_else(|| identity.get("settings").and_then(|v| v.get("Label"))),
        Some(Value::Null)
    )
}

/// Preserve explicit null even in fields whose usual absence is undefined.
/// The runtime reads both as None, but an oracle compares their tag bits.
pub fn encoded_content(
    content: &AiWorldContent,
    facts: &ContentFacts<'_>,
) -> Result<super::Call, AdmitError> {
    if content.classes.len() != facts.classes.len()
        || content.items.len() != facts.items.len()
        || content.posts.len() != facts.posts.len()
        || content.hulls.len() != facts.hulls.len()
        || content.props.len() != facts.props.len()
        || content
            .hulls
            .iter()
            .zip(&facts.hulls)
            .any(|(row, facts)| row.seats.len() != facts.seats.len())
        || content
            .props
            .iter()
            .zip(&facts.props)
            .any(|(row, facts)| row.target_policy.is_some() != facts.target_class.is_some())
    {
        return Err(error(
            "AI content rows do not match their identity receipts",
        ));
    }
    let mut call = content_call(content);
    for (i, class) in facts.classes.iter().enumerate() {
        if label_is_null(class.definition) {
            tag(
                &mut call,
                content,
                |v| v.classes[i].label = Some(StrId(0)),
                1.0,
            )?;
        }
    }
    for (i, item) in facts.items.iter().enumerate() {
        if matches!(item.definition.get("base"), Some(Value::Null)) {
            tag(
                &mut call,
                content,
                |v| v.items[i].base = Some(StrId(0)),
                1.0,
            )?;
        }
        if matches!(item.definition.get("class"), Some(Value::Null)) {
            tag(
                &mut call,
                content,
                |v| v.items[i].class_name = Some(StrId(0)),
                1.0,
            )?;
        }
    }
    for (i, post) in facts.posts.iter().enumerate() {
        if matches!(post.name, Some(Value::Null)) {
            tag(
                &mut call,
                content,
                |v| v.posts[i].name = Some(StrId(0)),
                1.0,
            )?;
        }
        if post.ai.get("order").is_none() {
            tag(&mut call, content, |v| v.posts[i].order = Some(0.0), 2.0)?;
        }
    }
    for (i, hull) in facts.hulls.iter().enumerate() {
        if label_is_null(hull.definition) {
            tag(
                &mut call,
                content,
                |v| v.hulls[i].policy.label = Some(StrId(0)),
                1.0,
            )?;
            for seat in 0..hull.seats.len() {
                tag(
                    &mut call,
                    content,
                    |v| v.hulls[i].seats[seat].policy.label = Some(StrId(0)),
                    1.0,
                )?;
            }
        }
    }
    for (i, prop) in facts.props.iter().enumerate() {
        if label_is_null(prop.desc) {
            tag(
                &mut call,
                content,
                |v| v.props[i].policy.label = Some(StrId(0)),
                1.0,
            )?;
            if prop.target_class.is_some() {
                tag(
                    &mut call,
                    content,
                    |v| {
                        if let Some(p) = &mut v.props[i].target_policy {
                            p.label = Some(StrId(0));
                        }
                    },
                    1.0,
                )?;
            }
        }
    }
    Ok(call)
}
