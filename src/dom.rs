use std::{borrow::Borrow, str};

use html5ever::{tendril::StrTendril, Attribute, LocalName, Namespace, QualName};
use jane_eyre::eyre;

pub fn attr_value<'attrs>(
    attrs: &'attrs [Attribute],
    name: &str,
) -> eyre::Result<Option<&'attrs str>> {
    for attr in attrs.iter() {
        if attr.name == QualName::new(None, Namespace::default(), LocalName::from(name)) {
            return Ok(Some(tendril_to_owned(&attr.value)?));
        }
    }

    Ok(None)
}

fn tendril_to_owned(tendril: &StrTendril) -> eyre::Result<&str> {
    Ok(str::from_utf8(tendril.borrow())?)
}
