use std::{borrow::Borrow, str};

use html5ever::{
    interface::{ElementFlags, TreeSink},
    local_name, namespace_url, ns,
    tendril::{StrTendril, TendrilSink},
    tree_builder::TreeBuilderOpts,
    Attribute, LocalName, Namespace, ParseOpts, QualName,
};
use jane_eyre::eyre::{self, bail};
use markup5ever_rcdom::{Handle, NodeData, RcDom, SerializableHandle};

pub struct Traverse(Vec<Handle>);

impl Traverse {
    pub fn new(node: Handle) -> Self {
        Self(vec![node])
    }
}

impl Iterator for Traverse {
    type Item = Handle;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }

        let node = self.0.remove(0);
        for kid in node.children.borrow().iter() {
            self.0.push(kid.clone());
        }

        Some(node)
    }
}

pub fn parse(mut input: &[u8]) -> eyre::Result<RcDom> {
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

pub fn serialize(dom: RcDom) -> eyre::Result<String> {
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
    let html_root: SerializableHandle = children[0].clone().into();

    let mut result = Vec::default();
    html5ever::serialize(&mut result, &html_root, Default::default())?;
    let result = String::from_utf8(result)?;

    Ok(result)
}

/// create a [`RcDom`] whose document has exactly one child, a wrapper <html> element.
pub fn create_fragment() -> (RcDom, Handle) {
    let mut dom = RcDom::default();
    let root = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(root.clone());

    (dom, root)
}

pub fn create_element(dom: &mut RcDom, html_local_name: &str) -> Handle {
    let name = QualName::new(None, ns!(html), LocalName::from(html_local_name));
    dom.create_element(name, vec![], ElementFlags::default())
}

pub fn find_attr_mut<'attrs>(
    attrs: &'attrs mut [Attribute],
    name: &str,
) -> Option<&'attrs mut Attribute> {
    for attr in attrs.iter_mut() {
        if attr.name == QualName::new(None, Namespace::default(), LocalName::from(name)) {
            return Some(attr);
        }
    }

    None
}

pub fn attr_value<'attrs>(
    attrs: &'attrs [Attribute],
    name: &str,
) -> eyre::Result<Option<&'attrs str>> {
    for attr in attrs.iter() {
        if attr.name == QualName::new(None, Namespace::default(), LocalName::from(name)) {
            return Ok(Some(tendril_to_str(&attr.value)?));
        }
    }

    Ok(None)
}

pub fn tendril_to_str(tendril: &StrTendril) -> eyre::Result<&str> {
    Ok(str::from_utf8(tendril.borrow())?)
}

#[test]
fn test_serialize() -> eyre::Result<()> {
    assert_eq!(serialize(RcDom::default()).map_err(|_| ()), Err(()));

    let mut dom = RcDom::default();
    let html = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(html);
    assert_eq!(serialize(dom)?, "");

    let mut dom = RcDom::default();
    let html = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(html);
    let html = create_element(&mut dom, "html");
    dom.document.children.borrow_mut().push(html);
    assert_eq!(serialize(dom).map_err(|_| ()), Err(()));

    let mut dom = RcDom::default();
    let html = create_element(&mut dom, "p");
    dom.document.children.borrow_mut().push(html);
    assert_eq!(serialize(dom).map_err(|_| ()), Err(()));

    Ok(())
}
