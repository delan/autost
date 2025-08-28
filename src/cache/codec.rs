use std::hash::Hash;

use bincode::{
    de::{BorrowDecoder, Decoder},
    enc::Encoder,
    error::{DecodeError, EncodeError},
    BorrowDecode, Decode, Encode,
};
use dashmap::DashMap;

use crate::cache::CachePack;

impl Decode<()> for CachePack {
    fn decode<D: Decoder<Context = ()>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let read_file_derivation_cache = DashMapDecoder::decode(decoder)?.0;
        let read_file_output_cache = DashMapDecoder::decode(decoder)?.0;
        let render_markdown_derivation_cache = DashMapDecoder::decode(decoder)?.0;
        let render_markdown_output_cache = DashMapDecoder::decode(decoder)?.0;
        let filtered_post_derivation_cache = DashMapDecoder::decode(decoder)?.0;
        let filtered_post_output_cache = DashMapDecoder::decode(decoder)?.0;
        let thread_derivation_cache = DashMapDecoder::decode(decoder)?.0;
        let thread_output_cache = DashMapDecoder::decode(decoder)?.0;
        let tag_index_derivation_cache = DashMapDecoder::decode(decoder)?.0;
        let tag_index_output_cache = DashMapDecoder::decode(decoder)?.0;
        let rendered_thread_derivation_cache = DashMapDecoder::decode(decoder)?.0;
        let rendered_thread_output_cache = DashMapDecoder::decode(decoder)?.0;

        Ok(Self {
            read_file_derivation_cache,
            read_file_output_cache,
            render_markdown_derivation_cache,
            render_markdown_output_cache,
            filtered_post_derivation_cache,
            filtered_post_output_cache,
            thread_derivation_cache,
            thread_output_cache,
            tag_index_derivation_cache,
            tag_index_output_cache,
            rendered_thread_derivation_cache,
            rendered_thread_output_cache,
        })
    }
}

impl<'__de> BorrowDecode<'__de, ()> for CachePack {
    fn borrow_decode<D: BorrowDecoder<'__de, Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        let read_file_derivation_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let read_file_output_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let render_markdown_derivation_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let render_markdown_output_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let filtered_post_derivation_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let filtered_post_output_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let thread_derivation_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let thread_output_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let tag_index_derivation_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let tag_index_output_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let rendered_thread_derivation_cache = DashMapDecoder::borrow_decode(decoder)?.0;
        let rendered_thread_output_cache = DashMapDecoder::borrow_decode(decoder)?.0;

        Ok(Self {
            read_file_derivation_cache,
            read_file_output_cache,
            render_markdown_derivation_cache,
            render_markdown_output_cache,
            filtered_post_derivation_cache,
            filtered_post_output_cache,
            thread_derivation_cache,
            thread_output_cache,
            tag_index_derivation_cache,
            tag_index_output_cache,
            rendered_thread_derivation_cache,
            rendered_thread_output_cache,
        })
    }
}

impl Encode for CachePack {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        DashMapEncoder(&self.read_file_derivation_cache).encode(encoder)?;
        DashMapEncoder(&self.read_file_output_cache).encode(encoder)?;
        DashMapEncoder(&self.render_markdown_derivation_cache).encode(encoder)?;
        DashMapEncoder(&self.render_markdown_output_cache).encode(encoder)?;
        DashMapEncoder(&self.filtered_post_derivation_cache).encode(encoder)?;
        DashMapEncoder(&self.filtered_post_output_cache).encode(encoder)?;
        DashMapEncoder(&self.thread_derivation_cache).encode(encoder)?;
        DashMapEncoder(&self.thread_output_cache).encode(encoder)?;
        DashMapEncoder(&self.tag_index_derivation_cache).encode(encoder)?;
        DashMapEncoder(&self.tag_index_output_cache).encode(encoder)?;
        DashMapEncoder(&self.rendered_thread_derivation_cache).encode(encoder)?;
        DashMapEncoder(&self.rendered_thread_output_cache).encode(encoder)?;

        Ok(())
    }
}

#[repr(transparent)]
struct DashMapDecoder<K: Eq + Hash, V, S>(DashMap<K, V, S>);

#[repr(transparent)]
struct DashMapEncoder<'inner, K: Eq + Hash, V, S>(&'inner DashMap<K, V, S>);

// <https://docs.rs/crate/bincode/2.0.1/source/src/features/impl_std.rs#449>
impl<
        '__de,
        K: BorrowDecode<'__de, ()> + Decode<()> + Encode + Eq + Hash,
        V: BorrowDecode<'__de, ()> + Decode<()> + Encode,
        S: std::hash::BuildHasher + Clone + Default,
    > Decode<()> for DashMapDecoder<K, V, S>
{
    fn decode<D: Decoder<Context = ()>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let len = decode_slice_len(decoder)?;
        decoder.claim_container_read::<(K, V)>(len)?;

        let hasher = Default::default();
        let result = Self(DashMap::with_capacity_and_hasher(len, hasher));
        for _ in 0..len {
            decoder.unclaim_bytes_read(core::mem::size_of::<(K, V)>());
            let key = K::decode(decoder)?;
            let value = V::decode(decoder)?;
            result.0.insert(key, value);
        }

        Ok(result)
    }
}

// <https://docs.rs/crate/bincode/2.0.1/source/src/features/impl_std.rs#472>
impl<
        '__de,
        K: BorrowDecode<'__de, ()> + Decode<()> + Encode + Eq + Hash,
        V: BorrowDecode<'__de, ()> + Decode<()> + Encode,
        S: std::hash::BuildHasher + Clone + Default,
    > BorrowDecode<'__de, ()> for DashMapDecoder<K, V, S>
{
    fn borrow_decode<D: BorrowDecoder<'__de, Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        let len = decode_slice_len(decoder)?;
        decoder.claim_container_read::<(K, V)>(len)?;

        let hasher = Default::default();
        let result = Self(DashMap::with_capacity_and_hasher(len, hasher));
        for _ in 0..len {
            decoder.unclaim_bytes_read(core::mem::size_of::<(K, V)>());
            let key = K::borrow_decode(decoder)?;
            let value = V::borrow_decode(decoder)?;
            result.0.insert(key, value);
        }

        Ok(result)
    }
}

// <https://docs.rs/crate/bincode/2.0.1/source/src/features/impl_std.rs#434>
impl<
        '__de,
        'inner,
        K: BorrowDecode<'__de, ()> + Decode<()> + Encode + Eq + Hash,
        V: BorrowDecode<'__de, ()> + Decode<()> + Encode,
        S: std::hash::BuildHasher + Clone + Default,
    > Encode for DashMapEncoder<'inner, K, V, S>
{
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        encode_slice_len(encoder, self.0.len())?;
        for pair in self.0.iter() {
            K::encode(pair.key(), encoder)?;
            V::encode(pair.value(), encoder)?;
        }
        Ok(())
    }
}

// <https://docs.rs/crate/bincode/2.0.1/source/src/de/mod.rs#328>
/// Decodes the length of any slice, container, etc from the decoder
#[inline]
fn decode_slice_len<D: Decoder>(decoder: &mut D) -> Result<usize, DecodeError> {
    let v = u64::decode(decoder)?;

    v.try_into().map_err(|_| DecodeError::OutsideUsizeRange(v))
}

// <https://docs.rs/crate/bincode/2.0.1/source/src/enc/mod.rs#99>
/// Encodes the length of any slice, container, etc into the given encoder
#[inline]
fn encode_slice_len<E: Encoder>(encoder: &mut E, len: usize) -> Result<(), EncodeError> {
    (len as u64).encode(encoder)
}
