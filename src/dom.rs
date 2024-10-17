use std::{
    borrow::Borrow,
    cell::{Ref, RefMut},
    collections::{BTreeMap, BTreeSet, VecDeque},
    str,
    sync::{LazyLock, Mutex},
};

use html5ever::{
    interface::{ElementFlags, TreeSink},
    local_name, namespace_url, ns,
    tendril::{StrTendril, TendrilSink},
    tree_builder::TreeBuilderOpts,
    Attribute, LocalName, Namespace, ParseOpts,
};
use jane_eyre::eyre::{self, bail};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};
use serde_json::Value;
use tracing::{error, warn};
use xml5ever::driver::XmlParseOpts;

pub use html5ever::QualName;

static ATTRIBUTES_SEEN: Mutex<BTreeSet<(String, String)>> = Mutex::new(BTreeSet::new());
static NOT_KNOWN_GOOD_ATTRIBUTES_SEEN: Mutex<BTreeSet<(String, String)>> =
    Mutex::new(BTreeSet::new());
static KNOWN_GOOD_ATTRIBUTES: LazyLock<BTreeSet<(Option<&'static str>, &'static str)>> =
    LazyLock::new(|| {
        let mut result = BTreeSet::default();
        result.insert((None, "aria-hidden"));
        result.insert((None, "aria-label"));
        result.insert((None, "id"));
        result.insert((None, "style"));
        result.insert((None, "tabindex"));
        result.insert((None, "title"));
        result.insert((Some("Mention"), "handle"));
        result.insert((Some("a"), "href"));
        result.insert((Some("a"), "name"));
        result.insert((Some("a"), "target"));
        result.insert((Some("details"), "name"));
        result.insert((Some("details"), "open"));
        result.insert((Some("div"), "align"));
        result.insert((Some("h3"), "align"));
        result.insert((Some("img"), "alt"));
        result.insert((Some("img"), "border"));
        result.insert((Some("img"), "height"));
        result.insert((Some("img"), "src"));
        result.insert((Some("img"), "width"));
        result.insert((Some("input"), "disabled"));
        result.insert((Some("input"), "name"));
        result.insert((Some("input"), "type"));
        result.insert((Some("input"), "value"));
        result.insert((Some("ol"), "start"));
        result.insert((Some("p"), "align"));
        result.insert((Some("td"), "align"));
        result.insert((Some("th"), "align"));
        result
    });
static RENAME_IDL_TO_CONTENT_ATTRIBUTE: LazyLock<
    BTreeMap<(Option<&'static str>, &'static str), &'static str>,
> = LazyLock::new(|| {
    let mut result = BTreeMap::default();
    result.insert((None, "ariaHidden"), "aria-hidden");
    result.insert((None, "ariaLabel"), "aria-label");
    result.insert((None, "className"), "class");
    result.insert((None, "tabIndex"), "tabindex");
    result
});

static HTML_ATTRIBUTES_WITH_URLS: LazyLock<BTreeMap<QualName, BTreeSet<QualName>>> =
    LazyLock::new(|| {
        BTreeMap::from([
            (
                QualName::html("a"),
                BTreeSet::from([QualName::attribute("href")]),
            ),
            (
                QualName::html("audio"),
                BTreeSet::from([QualName::attribute("src")]),
            ),
            (
                QualName::html("base"),
                BTreeSet::from([QualName::attribute("href")]),
            ),
            (
                QualName::html("button"),
                BTreeSet::from([QualName::attribute("formaction")]),
            ),
            (
                QualName::html("img"),
                BTreeSet::from([QualName::attribute("src")]),
            ),
            (
                QualName::html("form"),
                BTreeSet::from([QualName::attribute("action")]),
            ),
            (
                QualName::html("link"),
                BTreeSet::from([QualName::attribute("href")]),
            ),
            (
                QualName::html("script"),
                BTreeSet::from([QualName::attribute("src")]),
            ),
        ])
    });
static HTML_ATTRIBUTES_WITH_EMBEDDING_URLS: LazyLock<BTreeMap<QualName, BTreeSet<QualName>>> =
    LazyLock::new(|| {
        BTreeMap::from([
            (
                QualName::html("audio"),
                BTreeSet::from([QualName::attribute("src")]),
            ),
            (
                QualName::html("img"),
                BTreeSet::from([QualName::attribute("src")]),
            ),
        ])
    });
static HTML_ATTRIBUTES_WITH_NON_EMBEDDING_URLS: LazyLock<BTreeMap<QualName, BTreeSet<QualName>>> =
    LazyLock::new(|| {
        let mut result = HTML_ATTRIBUTES_WITH_URLS.clone();
        for (other_name, other_attr_names) in HTML_ATTRIBUTES_WITH_EMBEDDING_URLS.iter() {
            let attr_names = result
                .get_mut(other_name)
                .expect("guaranteed by constant values");
            for other_attr_name in other_attr_names.iter() {
                attr_names.remove(other_attr_name);
            }
        }
        result
    });

pub struct Traverse {
    queue: VecDeque<Handle>,
    elements_only: bool,
}
impl Traverse {
    pub fn nodes(node: Handle) -> Self {
        Self {
            queue: VecDeque::from([node]),
            elements_only: false,
        }
    }

    pub fn elements(node: Handle) -> Self {
        Self {
            queue: VecDeque::from([node]),
            elements_only: true,
        }
    }
}
impl Iterator for Traverse {
    type Item = Handle;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.queue.pop_front() {
            for kid in node.children.borrow().iter() {
                self.queue.push_back(kid.clone());
            }
            if !self.elements_only || matches!(node.data, NodeData::Element { .. }) {
                return Some(node);
            }
        }

        None
    }
}

pub struct Transform(VecDeque<Handle>);
impl Transform {
    pub fn new(node: Handle) -> Self {
        Self(VecDeque::from([node]))
    }

    pub fn next(
        &mut self,
        f: impl FnOnce(&[Handle], &mut Vec<Handle>) -> eyre::Result<()>,
    ) -> eyre::Result<bool> {
        if let Some(node) = self.0.pop_front() {
            let mut new_kids = vec![];
            f(&node.children.borrow(), &mut new_kids)?;
            for kid in new_kids.iter() {
                self.0.push_back(kid.clone());
            }
            node.children.replace(new_kids);
            Ok(!self.0.is_empty())
        } else {
            Ok(false)
        }
    }
}

pub trait HandleExt {
    fn attrs(&self) -> Option<RefMut<Vec<Attribute>>>;
}
impl HandleExt for Handle {
    fn attrs(&self) -> Option<RefMut<Vec<Attribute>>> {
        if let NodeData::Element { attrs, .. } = &self.data {
            Some(attrs.borrow_mut())
        } else {
            None
        }
    }
}

pub trait AttrsRefExt {
    fn attr_str(&self, name: &str) -> eyre::Result<Option<&str>>;
}
pub trait AttrsMutExt: AttrsRefExt {
    fn attr_mut(&mut self, name: &str) -> Option<&mut Attribute>;
}
impl AttrsMutExt for Vec<Attribute> {
    fn attr_mut(&mut self, name: &str) -> Option<&mut Attribute> {
        for attr in self.iter_mut() {
            if attr.name == QualName::attribute(name) {
                return Some(attr);
            }
        }

        None
    }
}
impl AttrsRefExt for Vec<Attribute> {
    fn attr_str(&self, name: &str) -> eyre::Result<Option<&str>> {
        for attr in self.iter() {
            if attr.name == QualName::attribute(name) {
                return Ok(Some(attr.value.to_str()));
            }
        }

        Ok(None)
    }
}
impl AttrsRefExt for Ref<'_, Vec<Attribute>> {
    fn attr_str(&self, name: &str) -> eyre::Result<Option<&str>> {
        (**self).attr_str(name)
    }
}
impl AttrsRefExt for RefMut<'_, Vec<Attribute>> {
    fn attr_str(&self, name: &str) -> eyre::Result<Option<&str>> {
        (**self).attr_str(name)
    }
}
impl AttrsMutExt for RefMut<'_, Vec<Attribute>> {
    fn attr_mut(&mut self, name: &str) -> Option<&mut Attribute> {
        (**self).attr_mut(name)
    }
}

pub trait TendrilExt: Borrow<[u8]> {
    fn to_str(&self) -> &str {
        str::from_utf8(self.borrow()).expect("only implemented by Tendril<UTF8>")
    }
}
impl TendrilExt for StrTendril {}

pub trait QualNameExt {
    fn html(name: &str) -> QualName {
        QualName::new(None, ns!(html), LocalName::from(name))
    }

    fn atom(name: &str) -> QualName {
        QualName::new(
            None,
            Namespace::from("http://www.w3.org/2005/Atom"),
            LocalName::from(name),
        )
    }

    fn attribute(name: &str) -> QualName {
        // per html5ever::Attribute docs:
        // “The namespace on the attribute name is almost always ns!(“”). The tokenizer creates all
        // attributes this way, but the tree builder will adjust certain attribute names inside foreign
        // content (MathML, SVG).”
        QualName::new(None, ns!(), LocalName::from(name))
    }
}
impl QualNameExt for QualName {}

pub fn parse_html_fragment(mut input: &[u8]) -> eyre::Result<RcDom> {
    let options = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let context = QualName::new(None, ns!(html), local_name!("section"));
    let dom = html5ever::parse_fragment(RcDom::default(), options, context, vec![])
        .from_utf8()
        .read_from(&mut input)?;

    Ok(dom)
}

pub fn parse_html_document(mut input: &[u8]) -> eyre::Result<RcDom> {
    let dom = html5ever::parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .read_from(&mut input)?;

    Ok(dom)
}

pub fn parse_xml(mut input: &[u8]) -> eyre::Result<RcDom> {
    let dom = xml5ever::driver::parse_document(RcDom::default(), XmlParseOpts::default())
        .from_utf8()
        .read_from(&mut input)?;

    Ok(dom)
}

pub fn serialize_html_document(dom: RcDom) -> eyre::Result<String> {
    serialize_node_contents(dom.document.clone())
}

pub fn serialize_html_fragment(dom: RcDom) -> eyre::Result<String> {
    // html5ever::parse_fragment builds a tree with the input wrapped in an <html> element.
    // this is consistent with how the web platform dom requires exactly one root element.
    let children = dom.document.children.borrow();
    if children.len() != 1 {
        bail!(
            "expected exactly one root element but got {}",
            children.len()
        );
    }
    let html = QualName::new(None, ns!(html), local_name!("html"));
    if !matches!(&children[0].data, NodeData::Element { name, .. } if name == &html) {
        bail!("expected root element to be <html>");
    }

    serialize_node_contents(children[0].clone())
}

pub fn serialize_node_contents(node: Handle) -> eyre::Result<String> {
    let mut result = Vec::default();
    let node: SerializableHandle = node.clone().into();
    // default SerializeOpts has `traversal_scope: ChildrenOnly(None)`.
    html5ever::serialize(&mut result, &node, Default::default())?;
    let result = String::from_utf8(result)?;

    Ok(result)
}

#[test]
fn test_serialize() -> eyre::Result<()> {
    assert_eq!(
        serialize_html_fragment(RcDom::default()).map_err(|_| ()),
        Err(())
    );

    let mut dom = RcDom::default();
    let html = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(html);
    assert_eq!(serialize_html_fragment(dom)?, "");

    let mut dom = RcDom::default();
    let html = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(html);
    let html = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(html);
    assert_eq!(serialize_html_fragment(dom).map_err(|_| ()), Err(()));

    let mut dom = RcDom::default();
    let html = create_element(&mut dom, "p");
    dom.document.children.borrow_mut().push(html);
    assert_eq!(serialize_html_fragment(dom).map_err(|_| ()), Err(()));

    Ok(())
}

/// create a [`RcDom`] whose document has exactly one child, a wrapper <html> element.
pub fn create_fragment() -> (RcDom, Handle) {
    let mut dom = RcDom::default();
    let root = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(root.clone());

    (dom, root)
}

pub fn create_element(dom: &mut RcDom, html_local_name: &str) -> Handle {
    let name = QualName::html(html_local_name);
    dom.create_element(name, vec![], ElementFlags::default())
}

pub fn rename_idl_to_content_attribute(tag_name: &str, attribute_name: &str) -> QualName {
    let result = RENAME_IDL_TO_CONTENT_ATTRIBUTE
        .get_key_value(&(Some(tag_name), attribute_name))
        .or_else(|| RENAME_IDL_TO_CONTENT_ATTRIBUTE.get_key_value(&(None, attribute_name)))
        .map_or(attribute_name, |(_, name)| name);

    // to be extra cautious about converting attributes correctly, warn if we see attributes not on
    // our known-good list.
    ATTRIBUTES_SEEN
        .lock()
        .unwrap()
        .insert((tag_name.to_owned(), result.to_owned()));
    if !KNOWN_GOOD_ATTRIBUTES.contains(&(None, result))
        && !KNOWN_GOOD_ATTRIBUTES.contains(&(Some(tag_name), result))
    {
        warn!("saw attribute not on known-good-attributes list! check if output is correct for: <{tag_name} {result}>");
        NOT_KNOWN_GOOD_ATTRIBUTES_SEEN
            .lock()
            .unwrap()
            .insert((tag_name.to_owned(), result.to_owned()));
    }

    QualName::attribute(result)
}

#[test]

fn test_rename_idl_to_content_attribute() {
    assert_eq!(
        rename_idl_to_content_attribute("div", "tabIndex"),
        QualName::attribute("tabindex"),
    );
}

pub fn convert_idl_to_content_attribute(
    tag_name: &str,
    attribute_name: &str,
    value: Value,
) -> Option<Attribute> {
    if value == Value::Bool(false) {
        return None;
    }

    Some(Attribute {
        name: rename_idl_to_content_attribute(tag_name, attribute_name),
        value: match (tag_name, attribute_name, value) {
            (_, _, Value::String(value)) => value.into(),
            (_, _, Value::Number(value)) => value.to_string().into(),
            (_, _, Value::Bool(true)) => "".into(),
            (_, _, Value::Bool(false)) => return None,
            // idl arrays with space-separated content values.
            // TODO: is this correct?
            (_, "className" | "rel", Value::Array(values)) => values
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .into(),
            (_, _, value) => {
                error!(
                    r"unknown attribute value type for <{tag_name} {attribute_name}>: {value:?}"
                );
                return None;
            }
        },
    })
}

#[test]
fn test_convert_idl_to_content_attribute() {
    assert_eq!(
        convert_idl_to_content_attribute("div", "id", Value::String("foo".to_owned())),
        Some(Attribute {
            name: QualName::attribute("id"),
            value: "foo".into(),
        }),
    );
    assert_eq!(
        convert_idl_to_content_attribute("img", "width", Value::Number(13.into())),
        Some(Attribute {
            name: QualName::attribute("width"),
            value: "13".into(),
        }),
    );
    assert_eq!(
        convert_idl_to_content_attribute("details", "open", Value::Bool(true)),
        Some(Attribute {
            name: QualName::attribute("open"),
            value: "".into(),
        }),
    );
    assert_eq!(
        convert_idl_to_content_attribute("details", "open", Value::Bool(false)),
        None,
    );
    assert_eq!(
        convert_idl_to_content_attribute(
            "div",
            "className",
            Value::Array(vec!["foo".into(), "bar".into()]),
        ),
        Some(Attribute {
            name: QualName::attribute("class"),
            value: "foo bar".into(),
        }),
    );
}

pub fn text_content(node: Handle) -> eyre::Result<String> {
    let mut result = vec![];
    for node in Traverse::nodes(node) {
        if let NodeData::Text { contents } = &node.data {
            result.push(contents.borrow().to_str().to_owned());
        }
    }

    Ok(result.join(""))
}

pub fn debug_attributes_seen() -> Vec<(String, String)> {
    ATTRIBUTES_SEEN.lock().unwrap().iter().cloned().collect()
}

pub fn debug_not_known_good_attributes_seen() -> Vec<(String, String)> {
    NOT_KNOWN_GOOD_ATTRIBUTES_SEEN
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .collect()
}

pub fn html_attributes_with_urls() -> &'static BTreeMap<QualName, BTreeSet<QualName>> {
    &HTML_ATTRIBUTES_WITH_URLS
}

pub fn html_attributes_with_embedding_urls() -> &'static BTreeMap<QualName, BTreeSet<QualName>> {
    &HTML_ATTRIBUTES_WITH_EMBEDDING_URLS
}

pub fn html_attributes_with_non_embedding_urls() -> &'static BTreeMap<QualName, BTreeSet<QualName>>
{
    &HTML_ATTRIBUTES_WITH_NON_EMBEDDING_URLS
}
