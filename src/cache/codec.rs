use std::collections::HashMap;

use bincode::{
    de::{BorrowDecoder, Decoder},
    enc::Encoder,
    error::DecodeError,
    BorrowDecode, Decode, Encode,
};

use crate::cache::{mem::Lazy, CachePack};

impl Decode<()> for CachePack {
    fn decode<D: Decoder<Context = ()>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let read_file_derivation_cache = HashMap::decode(decoder)?;
        let read_file_output_cache = HashMap::decode(decoder)?;
        let render_markdown_derivation_cache = HashMap::decode(decoder)?;
        let render_markdown_output_cache = HashMap::decode(decoder)?;
        let filtered_post_derivation_cache = HashMap::decode(decoder)?;
        let filtered_post_output_cache = HashMap::decode(decoder)?;
        let thread_derivation_cache = HashMap::decode(decoder)?;
        let thread_output_cache = HashMap::decode(decoder)?;
        let tag_index_derivation_cache = HashMap::decode(decoder)?;
        let tag_index_output_cache = HashMap::decode(decoder)?;
        let rendered_thread_derivation_cache = HashMap::decode(decoder)?;
        let rendered_thread_output_cache = HashMap::decode(decoder)?;

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
        let read_file_derivation_cache = HashMap::borrow_decode(decoder)?;
        let read_file_output_cache = HashMap::borrow_decode(decoder)?;
        let render_markdown_derivation_cache = HashMap::borrow_decode(decoder)?;
        let render_markdown_output_cache = HashMap::borrow_decode(decoder)?;
        let filtered_post_derivation_cache = HashMap::borrow_decode(decoder)?;
        let filtered_post_output_cache = HashMap::borrow_decode(decoder)?;
        let thread_derivation_cache = HashMap::borrow_decode(decoder)?;
        let thread_output_cache = HashMap::borrow_decode(decoder)?;
        let tag_index_derivation_cache = HashMap::borrow_decode(decoder)?;
        let tag_index_output_cache = HashMap::borrow_decode(decoder)?;
        let rendered_thread_derivation_cache = HashMap::borrow_decode(decoder)?;
        let rendered_thread_output_cache = HashMap::borrow_decode(decoder)?;

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
        self.read_file_derivation_cache.encode(encoder)?;
        self.read_file_output_cache.encode(encoder)?;
        self.render_markdown_derivation_cache.encode(encoder)?;
        self.render_markdown_output_cache.encode(encoder)?;
        self.filtered_post_derivation_cache.encode(encoder)?;
        self.filtered_post_output_cache.encode(encoder)?;
        self.thread_derivation_cache.encode(encoder)?;
        self.thread_output_cache.encode(encoder)?;
        self.tag_index_derivation_cache.encode(encoder)?;
        self.tag_index_output_cache.encode(encoder)?;
        self.rendered_thread_derivation_cache.encode(encoder)?;
        self.rendered_thread_output_cache.encode(encoder)?;

        Ok(())
    }
}

impl<T: Decode<()> + Encode> Decode<()> for Lazy<T> {
    fn decode<D: Decoder<Context = ()>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let content = Decode::decode(decoder)?;

        Ok(Self::raw(content))
    }
}

impl<'__de, T: Decode<()> + Encode> BorrowDecode<'__de, ()> for Lazy<T> {
    fn borrow_decode<D: BorrowDecoder<'__de, Context = ()>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        let content = BorrowDecode::borrow_decode(decoder)?;

        Ok(Self::raw(content))
    }
}

impl<T: Decode<()> + Encode> Encode for Lazy<T> {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        self.content.encode(encoder)
    }
}
